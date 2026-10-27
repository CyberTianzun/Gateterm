"""Verify the sidebar update offer against mocked APIs and WebSocket traffic.

Build: npm --prefix clients/web run build
Run: python scripts/test-web-update.py
Requires Python Playwright and an installed Google Chrome.
"""
import functools
import json
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from playwright.sync_api import expect, sync_playwright

root = Path(__file__).resolve().parents[1]


class Quiet(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


server = ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Quiet, directory=str(root / 'clients/web/dist')))
threading.Thread(target=server.serve_forever, daemon=True).start()
init = r"""
class Socket {
 static OPEN=1;
 constructor(){this.readyState=1;window.testSocket=this;setTimeout(()=>this.onopen?.({}),0);}
 emit(message){const body=new TextEncoder().encode(JSON.stringify(message));const frame=new Uint8Array(5+body.length);frame.set(body,5);this.onmessage?.({data:frame.buffer});}
 send(data){const msg=JSON.parse(new TextDecoder().decode(new Uint8Array(data).slice(5)));
  if(msg.type==='hello')setTimeout(()=>this.emit({type:'hello-ok',proto:1,os:'windows'}),0);
  if(msg.type==='list-sessions')setTimeout(()=>this.emit({type:'sessions',sessions:[]}),0);
 }
 close(){this.readyState=3;}
}
window.WebSocket=Socket;
"""
release_url = 'https://github.com/CyberTianzun/Gateterm/releases/tag/release-v0.3.0'
results = []
try:
    with sync_playwright() as p:
        browser = p.chromium.launch(channel='chrome', headless=True)
        for locale in ['en-US', 'zh-CN']:
            context = browser.new_context(locale=locale, viewport={'width': 1100, 'height': 760})
            context.add_init_script(init)
            page = context.new_page()
            page.clock.install()
            errors, credentials = [], []
            state = {'update': None, 'requests': 0, 'status': 200}
            page.on('pageerror', lambda error: errors.append(str(error)))

            def api(route):
                path = route.request.url.split('/api/', 1)[1].split('?', 1)[0]
                credentials.append(route.request.headers.get('authorization'))
                status = 200
                if path == 'update':
                    state['requests'] += 1
                    body = {'current_version': '0.2.3', 'update': state['update'], 'checked_at': 123}
                    status = state['status']
                elif path == 'cloud/status':
                    body = {'connected': True, 'email': 'test@example.com'}
                elif path == 'agents/sessions':
                    body = {'sessions': [], 'transcript_enabled': True}
                else:
                    body = {}
                route.fulfill(status=status, content_type='application/json', body=json.dumps(body))

            page.route('**/api/**', api)
            page.goto(f'http://127.0.0.1:{server.server_port}/#token=update-test')
            page.wait_for_function('document.querySelector("#cloud-state").dataset.connected === "true"')
            page.wait_for_timeout(100)
            assert state['requests'] == 1, state
            offer = page.locator('#update-status')
            expect(offer).to_be_hidden()
            state['update'] = {'version': '0.3.0', 'url': release_url}
            page.clock.fast_forward(60_000)
            expect(offer).to_be_visible()
            link = offer.locator('a')
            expect(link).to_have_attribute('href', release_url)
            expect(link).to_have_attribute('target', '_blank')
            expect(link).to_have_attribute('rel', 'noopener noreferrer')
            expect(link).to_have_text('发现新版本 v0.3.0，前往 GitHub ↗' if locale == 'zh-CN' else 'New version v0.3.0 on GitHub ↗')
            assert page.evaluate('document.querySelector("#update-status").previousElementSibling.id') == 'cloud-state'
            assert offer.bounding_box()['y'] > page.locator('#cloud-state').bounding_box()['y']
            context.route(release_url, lambda route: route.fulfill(status=200, content_type='text/html', body='<title>Release</title>'))
            with page.expect_popup() as opened:
                link.click()
            opened.value.wait_for_url(release_url)
            opened.value.close()
            # A failed local request preserves a known offer; a valid empty result clears it.
            state['status'] = 503
            before = state['requests']
            page.evaluate('window.dispatchEvent(new Event("focus"))')
            page.wait_for_timeout(100)
            assert state['requests'] > before
            expect(offer).to_be_visible()
            state.update(status=200, update=None)
            page.evaluate('window.dispatchEvent(new Event("focus"))')
            expect(offer).to_be_hidden()
            # Returning to the tab sees the next cache result without reloading.
            state['update'] = {'version': '0.3.0', 'url': release_url}
            page.evaluate('document.dispatchEvent(new Event("visibilitychange"))')
            expect(offer).to_be_visible()
            page.evaluate('window.testSocket.onclose({})')
            expect(offer).to_be_hidden()
            assert all(value == 'Bearer update-test' for value in credentials), credentials
            assert not errors, errors
            results.append({'locale': locale, 'checks': ['empty-hidden', 'poll', 'release-link', 'new-tab', 'network-error', 'clear', 'visibility', 'disconnect'], 'passed': True})
            context.close()
        browser.close()
finally:
    server.shutdown()
    server.server_close()
print(json.dumps(results))
