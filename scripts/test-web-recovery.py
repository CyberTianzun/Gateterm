"""Real Chrome/xterm regressions for terminal write/control recovery ordering.

Run `npm --prefix clients/web run build`, then `python scripts/test-web-recovery.py`.
Requires Python Playwright and Google Chrome. All API/WebSocket traffic is local.
"""
import functools
import json
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from playwright.sync_api import sync_playwright


class Quiet(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


root = Path(__file__).resolve().parents[1]
server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(
    Quiet, directory=str(root / "clients/web/dist")))
threading.Thread(target=server.serve_forever, daemon=True).start()
init = r"""
(() => {
localStorage.setItem('gateterm-config', JSON.stringify({version:1,ui:{webgl:false}}));
window.sent=[];
class Socket {
 static OPEN=1;
 constructor(){this.readyState=1;window.testSocket=this;setTimeout(()=>this.onopen?.({}),0);}
 emit(message,channel=0){const body=new TextEncoder().encode(channel?message:JSON.stringify(message));const frame=new Uint8Array(5+body.length);new DataView(frame.buffer).setUint32(0,channel,true);frame.set(body,5);this.onmessage?.({data:frame.buffer});}
 send(data){const bytes=new Uint8Array(data);const channel=new DataView(bytes.buffer).getUint32(0,true);if(channel)return;const msg=JSON.parse(new TextDecoder().decode(bytes.slice(5)));window.sent.push(msg);
 if(msg.type==='hello')setTimeout(()=>this.emit({type:'hello-ok',proto:1,os:'windows'}),0);
 if(msg.type==='list-sessions')setTimeout(()=>this.emit({type:'sessions',sessions:[{id:1,name:'Recovery 1',cols:20,rows:6,created_at:1},{id:2,name:'Recovery 2',cols:20,rows:6,created_at:1},{id:3,name:'Recovery 3',cols:20,rows:6,created_at:1}]}),0);
 if(msg.type==='attach'){
   window.channel=msg.channel;
   if(window.holdAttach)return;
   setTimeout(()=>{this.emit({type:'attached',channel:msg.channel,mode:'resync',seq:0});this.emit({type:'resized',channel:msg.channel,cols:20,rows:6});this.emit({type:'resync-begin',channel:msg.channel});this.emit('READY',msg.channel);this.emit({type:'resync-end',channel:msg.channel,seq:100});},0);
 }
 }
 close(){this.readyState=3;}
}
window.WebSocket=Socket;
window.burst=(events)=>{for(const event of events)window.testSocket.emit(event,typeof event==='string'?window.channel:0);};
})();
"""
try:
    with sync_playwright() as p:
        browser = p.chromium.launch(channel="chrome", headless=True)
        context = browser.new_context(viewport={"width": 1100, "height": 760}, locale="en-US")
        context.add_init_script(init)
        page = context.new_page()
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.route("**/api/**", lambda route: route.fulfill(
            status=404, content_type="application/json", body="{}"))
        page.goto(f"http://127.0.0.1:{server.server_port}/#token=browser-test")
        page.locator(".srow").first.click()
        page.wait_for_function("document.querySelector('.xterm-rows')?.textContent.includes('READY')")

        def rows():
            return page.locator(".xterm-rows > div").all_text_contents()

        # Old queued mode changes must finish before reset restores snapshot coordinates.
        page.evaluate(r"""() => {const c=window.channel;window.burst([
          '\x1b[?1049h\x1b[2;4r\x1b[?6h',
          {type:'resync-begin',channel:c},
          '\x1b[HHEADER\x1b[6;1HFOOTER',
          {type:'resync-end',channel:c,seq:200}
        ]);} """)
        page.wait_for_function("document.querySelector('.xterm-rows')?.textContent.includes('FOOTER')")
        screen = rows()
        assert screen[0].rstrip() == "HEADER", screen
        assert screen[5].rstrip() == "FOOTER", screen

        # Alternate-screen lines do not reflow: bytes queued at width 20 must wrap there.
        page.evaluate(r"""() => window.burst(['\x1b[?1049h\x1b[2J\x1b[H'])""")
        page.wait_for_timeout(80)
        page.evaluate(r"""() => window.burst([
          'AAAAAAAAAAAAAAAAAAAAZ',
          {type:'resized',channel:window.channel,cols:10,rows:6},
          '\x1b[2;1HEND'
        ])""")
        page.wait_for_function("document.querySelector('.xterm-rows')?.textContent.includes('END')")
        screen = rows()
        # xterm retains clipped alternate-buffer cells beyond the new right edge.
        assert screen[0].startswith("AAAAAAAAAA"), screen
        assert screen[1].rstrip() == "END", screen
        assert screen[2].strip() == "", screen

        # Old output and controls received before a replacement Attached cannot touch it.
        old_channel = page.evaluate("window.channel")
        page.evaluate("window.holdAttach=true")
        page.evaluate("""async () => {
          window.testSocket.emit('PENDING OLD OUTPUT',window.channel);
          await new Promise(resolve=>queueMicrotask(resolve));
          document.querySelectorAll('.srow')[1].click();
          document.querySelectorAll('.srow')[2].click();
        }""")
        new_channel = page.evaluate("window.channel")
        assert new_channel != old_channel, (old_channel, new_channel)
        page.evaluate(r"""({oldChannel,newChannel}) => {
          const s=window.testSocket;
          s.emit('STALE',oldChannel);
          s.emit({type:'resized',channel:oldChannel,cols:1,rows:1});
          s.emit({type:'resync-begin',channel:oldChannel});
          s.emit({type:'session-exited',channel:oldChannel,id:1});
          s.emit({type:'error',channel:oldChannel,message:'stale failure'});
          s.emit({type:'attached',channel:newChannel,mode:'resync',seq:0});
          s.emit({type:'resized',channel:newChannel,cols:20,rows:6});
          s.emit({type:'resync-begin',channel:newChannel});
          s.emit('CURRENT',newChannel);
          s.emit({type:'resync-end',channel:newChannel,seq:300});
        }""", {"oldChannel": old_channel, "newChannel": new_channel})
        page.wait_for_function("document.querySelector('.xterm-rows')?.textContent.includes('CURRENT')")
        assert rows()[0].rstrip() == "CURRENT", rows()
        assert not page.locator("#error-banner").is_visible()
        assert not errors, errors
        print(json.dumps({"reset_after_write": "passed", "resize_after_write": "passed",
                          "rapid_session_switch": "passed", "page_errors": errors}))
        browser.close()
finally:
    server.shutdown()
    server.server_close()
