"""Browser regression for the pinned xterm mouse adapter.

Build with `npm --prefix clients/web run build`, then run
`python scripts/test-web-mouse.py` from the repository root.
Requires the Python Playwright package and an installed Google Chrome; uses a
separate headless browser with fake WebSocket/API traffic and no agent process.
"""
import functools,json,re,threading
from pathlib import Path
from http.server import ThreadingHTTPServer,SimpleHTTPRequestHandler
from playwright.sync_api import sync_playwright
root=Path(__file__).resolve().parents[1]
class Quiet(SimpleHTTPRequestHandler):
 def log_message(self,*args): pass
server=ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Quiet,directory=str(root/'clients/web/dist')))
threading.Thread(target=server.serve_forever,daemon=True).start()
init=r"""
(() => {
window.mouseBytes=[];
class Socket {
 static OPEN=1;
 constructor(){this.readyState=1;window.testSocket=this;setTimeout(()=>this.onopen?.({}),0);}
 emit(message,channel=0){const body=new TextEncoder().encode(channel?message:JSON.stringify(message));const frame=new Uint8Array(5+body.length);new DataView(frame.buffer).setUint32(0,channel,true);frame.set(body,5);setTimeout(()=>this.onmessage?.({data:frame.buffer}),0);}
 send(data){const bytes=new Uint8Array(data);const channel=new DataView(bytes.buffer).getUint32(0,true);const text=new TextDecoder().decode(bytes.slice(5));if(channel){window.mouseBytes.push(text);return;}const msg=JSON.parse(text);
 if(msg.type==='hello')this.emit({type:'hello-ok',proto:1,os:'windows'});
 if(msg.type==='list-sessions')this.emit({type:'sessions',sessions:[{id:1,name:'Mouse test',cols:80,rows:24,created_at:1},{id:2,name:'Mouse test 2',cols:80,rows:24,created_at:1}]});
 if(msg.type==='attach'){this.emit({type:'resized',channel:1,cols:80,rows:24});this.emit('SELECT_ME 0123456789 mouse selection test\r\n\x1b[?1003h\x1b[?1006h',1);}
 }
 close(){this.readyState=3;}
}
window.WebSocket=Socket;
window.writeTerminal=(text)=>window.testSocket.emit(text,1);
})();
"""
with sync_playwright() as p:
 browser=p.chromium.launch(channel='chrome',headless=True)
 context=browser.new_context(viewport={'width':1100,'height':760},locale='en-US',permissions=['clipboard-read','clipboard-write'])
 context.add_init_script(init)
 page=context.new_page();errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 page.route('**/api/**',lambda route:route.fulfill(status=404,content_type='application/json',body='{}'))
 page.goto(f'http://127.0.0.1:{server.server_port}/#token=browser-test')
 page.locator('.srow').first.click();page.wait_for_selector('.xterm.enable-mouse-events')
 def output(): return page.evaluate('window.mouseBytes')
 def clear(): page.evaluate('window.mouseBytes=[]')
 def write(text): page.evaluate('(text)=>window.writeTerminal(text)',text);page.wait_for_timeout(80)
 def block(enabled):
  page.locator('#open-settings').click()
  page.get_by_role('button',name='Terminal',exact=True).click()
  page.get_by_role('checkbox',name=re.compile('^Block mouse events')).set_checked(enabled)
  page.locator('.settings-dialog .modal-close').click()
  page.wait_for_timeout(80)
 def drag():
  box=page.locator('.xterm-screen').bounding_box();x=box['x']+3;y=box['y']+box['height']/48
  page.mouse.move(x,y);page.mouse.down();page.mouse.move(x+box['width']*0.25,y,steps=12);page.mouse.up()
 def wheel(delta):
  box=page.locator('.xterm-screen').bounding_box();page.mouse.move(box['x']+50,box['y']+30);page.mouse.wheel(0,delta);page.wait_for_timeout(120)
 clear();drag();assert any('\x1b[<' in data for data in output()),output()
 block(True);assert page.locator('.xterm.enable-mouse-events').count()==0
 assert page.evaluate('JSON.parse(localStorage.getItem("gateterm-config")).terminal.blockMouseEvents') is True
 clear();page.locator('.xterm-helper-textarea').focus();page.keyboard.insert_text('KEYBOARD_OK');page.wait_for_timeout(80)
 assert any('KEYBOARD_OK' in data for data in output()),output()
 clear();drag();assert not output(),output()
 page.keyboard.press('Control+c')
 copied=page.evaluate('navigator.clipboard.readText()');assert 'SELECT_ME' in copied,copied
 # Reasserting tracking must not discard selection or activate mouse listeners.
 write('\x1b[?1000h\x1b[?1006h')
 assert page.locator('.xterm.enable-mouse-events').count()==0
 page.keyboard.press('Control+c');assert page.evaluate('navigator.clipboard.readText()')==copied
 # Alternate-buffer wheel events must not turn into arrow keys.
 write('\x1b[?1049hALT BUFFER\r\n\x1b[?1003h\x1b[?1006h');clear();wheel(120);assert not output(),output()
 # Normal-buffer history scrolling remains local.
 write('\x1b[?1049l'+''.join(f'History line {i}\r\n' for i in range(120)))
 old=page.locator('.xterm-viewport').evaluate('(node)=>node.scrollTop');clear();wheel(-320)
 new=page.locator('.xterm-viewport').evaluate('(node)=>node.scrollTop');assert new<old,(old,new);assert not output(),output()
 # Restoring the latest requested mode (VT200) must not restore the earlier ANY mode.
 write('\x1b[?1000h');block(False);clear()
 box=page.locator('.xterm-screen').bounding_box();page.mouse.move(box['x']+100,box['y']+40,steps=4);page.wait_for_timeout(80)
 assert not output(),output()
 page.mouse.click(box['x']+100,box['y']+40);assert any('\x1b[<' in data for data in output()),output()
 # A remote disable received while blocked remains disabled after unchecking.
 block(True);write('\x1b[?1000l');block(False);clear();drag();assert not output(),output()
 # With no scrollback even the normal buffer would otherwise synthesize arrows.
 block(True)
 page.locator('#open-settings').click()
 page.get_by_role('button',name='Terminal',exact=True).click()
 page.get_by_role('spinbutton',name=re.compile('^Scrollback')).fill('0')
 page.get_by_role('spinbutton',name=re.compile('^Scrollback')).press('Tab')
 page.locator('.settings-dialog .modal-close').click()
 clear();wheel(120);assert not output(),output()
 # A full terminal reset while blocked must not revive an obsolete mouse mode.
 write('\x1b[?1003h');write('\x1bc');block(False)
 assert page.locator('.xterm.enable-mouse-events').count()==0
 clear();drag();assert not output(),output()
 # Switching terminals must dispose the previous policy without affecting the next instance.
 block(True);page.locator('.srow').nth(1).click();page.wait_for_timeout(120)
 assert page.locator('.xterm.enable-mouse-events').count()==0
 clear();drag();assert not output(),output()
 # A fresh terminal after reload also starts blocked even if the program enables tracking.
 block(True);page.reload();page.locator('.srow').first.click();page.wait_for_selector('.xterm');page.wait_for_timeout(180)
 assert page.locator('.xterm.enable-mouse-events').count()==0
 clear();drag();assert not output(),output()
 assert not errors,errors
 print(json.dumps({'default_mouse_tracking':True,'local_selection_and_copy':copied,'remote_mode_reassertion':'blocked','alternate_buffer_wheel':'blocked','local_scrollback':'passed','latest_mode_restored':'passed','remote_disable_preserved':'passed','zero_scrollback_wheel':'blocked','terminal_reset':'passed','keyboard_input':'passed','session_switch':'passed','reload_persistence':'passed','page_errors':errors}))
 browser.close()
server.shutdown();server.server_close()
