"""Check one credential prompt per page across WebSocket and API consumers.

Build: npm --prefix clients/web run build
Run: python scripts/test-web-auth.py
Requires Python Playwright and an installed Google Chrome. All server traffic is mocked.
"""
import functools, json, threading
from pathlib import Path
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from playwright.sync_api import sync_playwright

root = Path(__file__).resolve().parents[1]
class Quiet(SimpleHTTPRequestHandler):
    def log_message(self, *args): pass
server = ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Quiet, directory=str(root/'clients/web/dist')))
threading.Thread(target=server.serve_forever, daemon=True).start()
init = r"""
(() => {
window.authHellos=[];
class Socket {
 static OPEN=1;
 constructor(){this.readyState=1;setTimeout(()=>this.onopen?.({}),0);}
 emit(msg){const body=new TextEncoder().encode(JSON.stringify(msg));const frame=new Uint8Array(5+body.length);frame.set(body,5);this.onmessage?.({data:frame.buffer});}
 send(data){const msg=JSON.parse(new TextDecoder().decode(new Uint8Array(data).slice(5)));
  if(msg.type==='hello'){window.authHellos.push(msg.token);setTimeout(()=>{
   if(!msg.token || msg.token==='invalid'){this.emit({type:'error',message:'Invalid token'});this.onclose?.({});}
   else this.emit({type:'hello-ok',proto:1,os:'windows'});
  },0);}
  if(msg.type==='list-sessions')setTimeout(()=>this.emit({type:'sessions',sessions:[]}),0);
 }
 close(){this.readyState=3;}
}
window.WebSocket=Socket;
})();
"""
with sync_playwright() as p:
    browser=p.chromium.launch(channel='chrome', headless=True)
    checks=[]
    for name, answer, blocked, shared in [
        ('manual', 'entered-once', False, None),
        ('storage-blocked', 'in-memory-token', True, None),
        ('shared-storage-blocked', None, True, 'shared-token'),
        ('cancelled', None, False, None),
        ('invalid-storage-blocked', 'invalid', True, None),
    ]:
        context=browser.new_context(locale='en-US')
        context.add_init_script(init)
        if blocked:
            context.add_init_script("for(const method of ['getItem','setItem','removeItem']) Storage.prototype[method]=()=>{throw new DOMException('Blocked','SecurityError')}")
        page=context.new_page(); prompts=[]; requests=[]; errors=[]
        page.on('pageerror', lambda error: errors.append(str(error)))
        def dialog(d):
            prompts.append(d.type)
            if answer is None: d.dismiss()
            else: d.accept(answer)
        page.on('dialog', dialog)
        def api(route):
            path=route.request.url.split('/api/',1)[1].split('?',1)[0]
            requests.append((path, route.request.headers.get('authorization')))
            if path=='cloud/status': body={'connected':False}
            elif path=='agents/sessions': body={'sessions':[], 'transcript_enabled':True}
            elif path=='agents/history': body={'sessions':[], 'agents':[], 'total':0, 'next_offset':None,'truncated':False,'skipped':0,'indexing':False}
            elif path=='agents': body={'agents':[]}
            elif path=='launchers': body={'launchers':[]}
            else: body={}
            route.fulfill(status=200, content_type='application/json', body=json.dumps(body))
        page.route('**/api/**', api)
        url=f'http://127.0.0.1:{server.server_port}/'+(f'#token={shared}' if shared else '')
        page.goto(url)
        page.wait_for_function('window.authHellos.length===1')
        expected=shared or answer or ''
        assert page.evaluate('window.authHellos[0]')==expected, name
        page.locator('#open-settings').click()
        page.get_by_role('button',name='Agents',exact=True).click()
        page.wait_for_timeout(100)
        page.locator('.settings-dialog .modal-close').click()
        page.locator('#open-history').click()
        page.wait_for_timeout(100)
        page.locator('.history-dialog .modal-close').click()
        page.locator('#new-session').click()
        page.wait_for_timeout(100)
        assert len(prompts)==(0 if shared else 1),(name,prompts)
        assert {'agents','agents/history','launchers'} <= {path for path,_ in requests},(name,requests)
        assert all(auth==f'Bearer {expected}'.rstrip() for _,auth in requests),(name,requests)
        if name=='manual':
            assert page.evaluate('localStorage.getItem("gateterm-token")')==answer
            page.reload();page.wait_for_function('window.authHellos.length===1')
            page.wait_for_timeout(100)
            assert len(prompts)==1,prompts
        assert not errors,(name,errors)
        checks.append({'case':name,'prompts':len(prompts),'api_requests':len(requests),'passed':True})
        context.close()
    browser.close()
server.shutdown();server.server_close()
print(json.dumps(checks))
