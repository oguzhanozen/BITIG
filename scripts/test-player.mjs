/* global fetch, WebSocket, console, setTimeout */
// Run against Vite on :1420 and an isolated Chromium/WebView2 CDP target on :9341.
import assert from 'node:assert/strict';
import { Buffer } from 'node:buffer';
import { writeFile } from 'node:fs/promises';
import process from 'node:process';

const port = process.env.CDP_PORT || '9335';
const [page] = await fetch(`http://127.0.0.1:${port}/json/list`).then(r => r.json());
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise(resolve => ws.addEventListener('open', resolve, { once: true }));
let sequence = 0;
const pending = new Map();
const exceptions = [];
ws.addEventListener('message', event => {
  const message = JSON.parse(event.data);
  if (message.method === 'Runtime.exceptionThrown') exceptions.push(message.params.exceptionDetails.exception?.description ?? JSON.stringify(message.params.exceptionDetails));
  if (message.id) {
    const task = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) task.reject(new Error(JSON.stringify(message.error)));
    else task.resolve(message.result);
  }
});
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true, userGesture: true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  return result.result.value;
}
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(expression) {
  for (let i = 0; i < 100; i++) {
    if (await evaluate(expression)) return;
    await pause(50);
  }
  throw new Error(`Timed out: ${expression}`);
}
async function key(key, code) {
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code });
}

async function checkLayout(label) {
  const geometry = await evaluate(`(() => {
    const root = document.querySelector('.media-player'), stage = root.querySelector('.player-stage'), video = root.querySelector('video'), bar = root.querySelector('.player-controls');
    const bounds = node => { const r = node.getBoundingClientRect(); return { x:r.x, y:r.y, right:r.right, bottom:r.bottom, width:r.width, height:r.height }; };
    const player = bounds(root), picture = bounds(video), surface = bounds(stage), controls = bounds(bar), form = root.closest('.media-detail'), formStyle = getComputedStyle(form);
    return { player, picture, surface, controls, fit:getComputedStyle(video).objectFit,
      ratio:video.videoWidth / video.videoHeight, viewport:{ width:innerWidth, height:innerHeight },
      overflow:[...bar.querySelectorAll('button,input')].map(bounds).filter(r => r.x < controls.x - 1 || r.right > controls.right + 1 || r.bottom > controls.bottom + 1),
      opacity:getComputedStyle(bar).opacity, fullscreen:document.fullscreenElement === root,
      theater:root.closest('dialog').classList.contains('media-theater'),
      availableHeight:form.clientHeight - parseFloat(formStyle.paddingTop) - parseFloat(formStyle.paddingBottom) };
  })()`);
  assert.ok(geometry.surface.height > 0 && geometry.picture.bottom <= geometry.controls.y + 1, label + ': video stays above controls');
  assert.ok(Math.abs(geometry.surface.bottom - geometry.controls.y) < 1, label + ': adjacent stage and controls');
  assert.ok(Math.abs(geometry.controls.bottom - geometry.player.bottom) <= 1, label + ': controls end at player bottom');
  assert.equal(geometry.fit, 'contain', label + ': preserve image ratio');
  assert.deepEqual(geometry.overflow, [], label + ': no clipped controls');
  assert.equal(geometry.opacity, '1', label + ': controls stay visible');
  if (geometry.theater && !geometry.fullscreen) assert.ok(Math.abs(geometry.player.height - geometry.availableHeight) <= 1, label + ': theater fills the available form height');
  if (geometry.fullscreen) {
    assert.equal(geometry.player.height, geometry.viewport.height, label + ': fullscreen fills viewport');
    assert.ok(Math.abs(geometry.controls.bottom - geometry.viewport.height) < 1, label + ': fullscreen controls at bottom');
  }
  return geometry;
}

try {
  await send('Runtime.enable');
  await send('Page.enable');
  await send('Emulation.setFocusEmulationEnabled', { enabled:true });
  await send('Emulation.setDeviceMetricsOverride', { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  await send('Page.addScriptToEvaluateOnNewDocument', { source: `
    localStorage.setItem('bitig.quickTour.completed', 'true');
    localStorage.setItem('bitig.theme', 'dark');
    localStorage.setItem('bitig.toolUpdates.auto', 'disabled');
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
      invoke: async (command) => {
        if (command === 'get_media_playback_path') return window.__sampleSrc;
        if (command === 'get_media_thumbnail_path') return window.__posterData;
        if (command.startsWith('list_')) return [];
        if (command.includes('is_maximized')) return false;
        return 1;
      },
      convertFileSrc: value => value,
      transformCallback: () => 1,
      unregisterCallback: () => {}
    };
  ` });
  await send('Page.navigate', { url: 'http://localhost:1420' });
  await until(`!!document.querySelector('.app-frame')`);
  await evaluate(`(async () => {
    const rate = 8000, size = rate * 120 * 2;
    const wav = new Uint8Array(44 + size), view = new DataView(wav.buffer);
    const text = (at, value) => [...value].forEach((letter, index) => wav[at + index] = letter.charCodeAt(0));
    text(0, 'RIFF'); view.setUint32(4, 36 + size, true); text(8, 'WAVE'); text(12, 'fmt ');
    view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
    view.setUint32(24, rate, true); view.setUint32(28, rate * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
    text(36, 'data'); view.setUint32(40, size, true);
    window.__sampleSrc = URL.createObjectURL(new Blob([wav], { type: 'audio/wav' }));
    const canvas = document.createElement('canvas'); canvas.width = 1280; canvas.height = 720;
    const context = canvas.getContext('2d'); context.fillStyle = '#101923'; context.fillRect(0, 0, 1280, 720);
    for (let y = 0; y < 720; y += 40) { context.fillStyle = y % 80 ? '#182737' : '#1b2d3d'; context.fillRect(0, y, 1280, 40); }
    context.fillStyle = '#bdddfc'; context.font = '32px monospace'; context.fillText('BITIG / LOCAL PLAYBACK', 80, 180);
    context.fillStyle = '#88bdf2'; context.fillRect(80, 230, 160, 4);
    context.fillStyle = '#3c586f'; context.beginPath(); context.moveTo(0, 600); context.lineTo(320, 320); context.lineTo(550, 520); context.lineTo(850, 280); context.lineTo(1280, 650); context.lineTo(1280, 720); context.lineTo(0, 720); context.fill();
    window.__posterData = canvas.toDataURL();
    const mainModule = await fetch('/src/main.tsx').then(response => response.text());
    const reactUrl = mainModule.split('"').find(part => part.includes('/react.js?'));
    const rendererUrl = mainModule.split('"').find(part => part.includes('/react-dom_client.js?'));
    const { default: React } = await import(reactUrl);
    const { createRoot } = (await import(rendererUrl)).default;
    const { MediaDetailDialog } = await import('/src/components/MediaDetailDialog.tsx');
    const mount = document.createElement('div'); document.body.append(mount);
    const root = createRoot(mount); window.__closed = 0;
    window.mountSample = (id, mediaType = 'video') => root.render(React.createElement(MediaDetailDialog, {
      key: id,
      media: { id, folderId: null, title: 'Coastal archive — 2026', mediaType, sourceUrl: '', sourcePlatform: 'test', sourceId: id, creator: null, filePath: '', thumbnailPath: 'poster.webp', container: 'webm', width: 1280, height: 720, durationMs: 120000, fileSize: size, createdAt: '', updatedAt: '' },
      folders: [], onClose: () => { window.__closed++; root.render(null); }, onChanged: () => {}, requestConfirmation: async () => false
    }));
    window.mountSample('first');
  })()`);
  await until(`document.querySelector('.media-player video')?.readyState >= 2`);
  await until(`!!document.querySelector('.media-player video')?.poster`);
  assert.equal(await evaluate(`document.querySelector('.media-player video').controls`), false);
  assert.equal(await evaluate(`document.querySelector('.media-player video').duration`), 120);
  const layout = await evaluate(`(() => {
    const root = document.querySelector('.media-player'), stage = root.querySelector('.player-stage'), controls = root.querySelector('.player-controls'), video = root.querySelector('video');
    const canvas = document.createElement('canvas'), context = canvas.getContext('2d');
    canvas.width = canvas.height = 1;
    context.fillStyle = getComputedStyle(controls).backgroundColor;
    context.fillRect(0, 0, 1, 1);
    return { siblings: stage?.parentElement === root && controls?.parentElement === root,
      videoInStage: !!stage?.contains(video), controlsOutsideStage: !!stage && !stage.contains(controls),
      separated: !!stage && video.getBoundingClientRect().bottom <= controls.getBoundingClientRect().top,
      controlsInFlow: getComputedStyle(controls).position !== 'absolute',
      opaque: context.getImageData(0, 0, 1, 1).data[3] === 255 };
  })()`);
  assert.equal(layout.siblings, true, 'video stage and controls must be sibling regions');
  assert.equal(layout.videoInStage && layout.controlsOutsideStage, true, 'video belongs only to the stage');
  assert.equal(layout.separated && layout.controlsInFlow && layout.opaque, true, 'opaque controls must occupy their own layout space below video: ' + JSON.stringify(layout));
  console.log('PASS separate video stage and opaque controls in actual browser layout');
  console.log('PASS custom controls, media load, finite duration, poster');

  await evaluate(`document.querySelector('button[aria-label="Play"]').click()`);
  await until(`document.querySelector('.media-player').dataset.state === 'playing'`);
  await evaluate(`document.querySelector('button[aria-label="Pause"]').click()`);
  await until(`document.querySelector('.media-player video').paused`);
  await evaluate(`document.querySelector('.media-player video').currentTime = 20`);
  await until(`document.querySelector('.player-seek').valueAsNumber === 20`);
  await evaluate(`document.querySelector('button[aria-label="Forward 10 seconds"]').click()`);
  await until(`document.querySelector('.media-player video').currentTime === 30`);
  await evaluate(`document.querySelector('button[aria-label="Back 10 seconds"]').click()`);
  await until(`document.querySelector('.media-player video').currentTime === 20`);
  await evaluate(`document.querySelector('.media-player').focus()`);
  await key('ArrowLeft', 37);
  await until(`document.querySelector('.media-player video').currentTime === 10`);
  await key('l', 76);
  await until(`document.querySelector('.media-player video').currentTime === 20`);
  console.log('PASS play/pause, skip buttons and scoped seek shortcuts');

  await evaluate(`document.querySelector('.media-detail label input').focus()`);
  await key('k', 75);
  await key('ArrowLeft', 37);
  assert.equal(await evaluate(`document.querySelector('.media-player video').paused && document.querySelector('.media-player video').currentTime === 20`), true);
  await evaluate(`document.querySelector('.media-player').focus()`);
  await key('m', 77);
  await until(`document.querySelector('.media-player video').muted`);
  await key('m', 77);
  await until(`!document.querySelector('.media-player video').muted`);
  await evaluate(`document.querySelector('.media-player button[aria-label="Playback speed"]').click()`);
  await evaluate(`[...document.querySelectorAll('[role="option"]')].find(node => node.textContent === '1.5×').click()`);
  await until(`document.querySelector('.media-player video').playbackRate === 1.5`);
  console.log('PASS editing exclusion, mute shortcuts, speed selection');
  await evaluate(`(() => { const range = document.querySelector('input[aria-label="Volume"]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(range, '0.25'); range.dispatchEvent(new Event('input', { bubbles: true })); })()`);
  await until(`document.querySelector('.media-player video').volume === 0.25`);
  await evaluate(`(() => { const range = document.querySelector('.player-seek'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(range, '40'); range.dispatchEvent(new Event('input', { bubbles: true })); })()`);
  await until(`document.querySelector('.media-player video').currentTime === 40`);
  await evaluate(`document.querySelector('.player-seek').focus()`);
  await key('ArrowRight', 39);
  await until(`Math.abs(document.querySelector('video').currentTime - 40.1) < 0.001`);
  await evaluate(`document.querySelector('input[aria-label="Volume"]').focus()`);
  await key('ArrowRight', 39);
  await until(`Math.abs(document.querySelector('video').volume - 0.26) < 0.001`);
  await until(`document.activeElement.matches(':focus-visible') && getComputedStyle(document.activeElement).outlineWidth === '2px'`);
  assert.equal(await evaluate(`getComputedStyle(document.activeElement).outlineWidth`), '2px');
  await evaluate(`document.querySelector('.media-player video').currentTime = 20`);
  await until(`!document.querySelector('.media-player video').seeking`);
  assert.equal(await evaluate(`document.querySelector('.player-seek').getBoundingClientRect().height`), 28);
  assert.equal(await evaluate(`document.querySelector('.media-player .app-select-trigger').getBoundingClientRect().width`), 60);
  assert.equal(await evaluate(`document.querySelector('input[aria-label="Volume"]').getBoundingClientRect().width`), 64);
  console.log('PASS native volume/seek sliders, keyboard steps, visible focus and control sizes');

  await evaluate(`window.__video = document.querySelector('.media-player video'); document.querySelector('button[aria-label="Theater Mode"]').click()`);
  await until(`document.querySelector('dialog').classList.contains('media-theater')`);
  assert.equal(await evaluate(`document.querySelector('.media-player video') === window.__video && window.__video.currentTime === 20`), true);
  await pause(3100);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false, 'Theater Mode must never auto-hide controls');
  const theater = await send('Page.captureScreenshot', { format: 'png' });
  await writeFile('node_modules/.tmp/player-redesign-theater.png', Buffer.from(theater.data, 'base64'));
  await evaluate(`document.querySelector('button[aria-label="Fullscreen"]').click()`);
  await until(`document.fullscreenElement === document.querySelector('.media-player')`);
  await until(`!!document.querySelector('button[aria-label="Exit fullscreen"]')`);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false, 'Fullscreen must start with visible controls');
  assert.equal(await evaluate(`getComputedStyle(document.querySelector('.player-controls')).outlineStyle`), 'none', 'Fullscreen entry must not draw focus outline around control bar');
  assert.equal(await evaluate(`(() => { const r = document.querySelector('.media-player'); return r.clientWidth === innerWidth && r.clientHeight === innerHeight && r.scrollWidth <= innerWidth && r.scrollHeight <= innerHeight; })()`), true, 'Fullscreen player must not exceed viewport bounds');
  await evaluate(`document.querySelector('button[aria-label="Exit fullscreen"]').focus()`);
  await key('Tab', 9);
  assert.equal(await evaluate(`document.activeElement === document.querySelector('input[aria-label="Playback position"]')`), true);
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', windowsVirtualKeyCode: 9, modifiers: 8 });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', windowsVirtualKeyCode: 9, modifiers: 8 });
  assert.equal(await evaluate(`document.activeElement === document.querySelector('button[aria-label="Exit fullscreen"]')`), true);
  await pause(3100);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false, 'Controls must not auto-hide while keyboard focus is inside controls');
  await evaluate(`document.querySelector('.media-player button[aria-label="Playback speed"]').click()`);
  assert.equal(await evaluate(`document.fullscreenElement.contains(document.querySelector('[role="listbox"]'))`), true);
  await until(`(() => { const menu = document.querySelector('[role="listbox"]'); if (!menu) return false; const r = menu.getBoundingClientRect(); return r.height > 0 && r.top >= 0 && r.bottom <= innerHeight && menu.contains(document.elementFromPoint(r.x + r.width / 2, r.y + 20)); })()`);
  await pause(3100);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false, 'Controls must not auto-hide while speed menu is open');
  await evaluate(`[...document.querySelectorAll('[role="option"]')].find(node => node.textContent === '1×').click()`);
  await evaluate(`document.querySelector('.media-player').focus()`);
  await evaluate(`document.querySelector('.player-seek').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))`);
  await pause(3100);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false, 'Controls must not auto-hide during active seek/volume interaction');
  await evaluate(`window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }))`);
  await pause(1800);
  const fsBox = await evaluate(`(() => { const r = document.querySelector('.media-player').getBoundingClientRect(); return { x: r.x + 60, y: r.y + 80 }; })()`);
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...fsBox });
  await pause(1800);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false, 'Mouse movement must reset the 3s inactivity timer');
  await until(`document.querySelector('.media-player').classList.contains('controls-hidden')`);
  const hiddenPausedMetrics = await evaluate(`(() => {
    const root = document.querySelector('.media-player'), stage = root.querySelector('.player-stage'), video = root.querySelector('video'), controls = root.querySelector('.player-controls');
    const sr = stage.getBoundingClientRect(), vr = video.getBoundingClientRect(), cr = controls.getBoundingClientRect(), cs = getComputedStyle(controls);
    const hit = document.elementFromPoint(innerWidth / 2, innerHeight - 10);
    return { stageH: sr.height, stageBottom: sr.bottom, videoH: vr.height, videoBottom: vr.bottom, controlsH: cr.height, opacity: cs.opacity, pointerEvents: cs.pointerEvents, hitClass: hit?.className || hit?.tagName };
  })()`);
  assert.equal(hiddenPausedMetrics.opacity, '0');
  assert.equal(hiddenPausedMetrics.pointerEvents, 'none');
  assert.equal(hiddenPausedMetrics.controlsH, 0, 'Hidden controls must not occupy layout height');
  assert.equal(hiddenPausedMetrics.stageH, 800, 'Video stage must fill full fullscreen height when controls are hidden');
  assert.equal(hiddenPausedMetrics.videoH, 800, 'Video element must fill full fullscreen height when controls are hidden');
  assert.equal(hiddenPausedMetrics.hitClass, 'player-surface', 'Hidden controls must not block pointer clicks at bottom of screen');
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: fsBox.x + 15, y: fsBox.y + 15 });
  await until(`!document.querySelector('.media-player').classList.contains('controls-hidden') && getComputedStyle(document.querySelector('.player-controls')).opacity === '1'`);
  assert.equal(await evaluate(`getComputedStyle(document.querySelector('.player-controls')).opacity`), '1');
  await evaluate(`document.querySelector('button[aria-label="Play"]').click()`);
  await until(`!document.querySelector('video').paused`);
  await until(`document.querySelector('.media-player').classList.contains('controls-hidden')`);
  assert.equal(await evaluate(`getComputedStyle(document.querySelector('.player-controls')).opacity`), '0');
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: fsBox.x + 30, y: fsBox.y + 30 });
  await until(`!document.querySelector('.media-player').classList.contains('controls-hidden') && getComputedStyle(document.querySelector('.player-controls')).opacity === '1'`);
  await evaluate(`document.querySelector('button[aria-label="Pause"]').click()`);
  await evaluate(`document.querySelector('.media-player').focus()`);
  await key('Escape', 27);
  await until(`!document.fullscreenElement`);
  assert.equal(await evaluate(`!document.querySelector('.media-player').classList.contains('controls-hidden') && window.__closed === 0 && document.querySelector('dialog').open && window.__video === document.querySelector('.media-player video')`), true);
  await pause(300);
  await key('Escape', 27);
  await until(`!document.querySelector('dialog').classList.contains('media-theater')`);
  console.log('PASS theater node/position preservation, fullscreen auto-hide, fullscreen containment, Escape isolation');

  await evaluate(`document.querySelector('.media-player').focus()`);
  await key('k', 75);
  await until(`!document.querySelector('.media-player video').paused`);
  await pause(3100);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false);
  assert.equal(await evaluate(`getComputedStyle(document.querySelector('.player-controls')).opacity`), '1');
  const box = await evaluate(`(() => { const r = document.querySelector('.media-player').getBoundingClientRect(); return { x: r.x + 30, y: r.y + 60 }; })()`);
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...box });
  await until(`!document.querySelector('.media-player').classList.contains('controls-hidden')`);
  await evaluate(`document.querySelector('button[aria-label="Pause"]').focus()`);
  await pause(3000);
  assert.equal(await evaluate(`document.querySelector('.media-player').classList.contains('controls-hidden')`), false);
  await key(' ', 32);
  await until(`document.querySelector('.media-player video').paused`);
  console.log('PASS controls remain visible while idle, focus retention, single Space activation');

  await evaluate(`Object.defineProperty(window.__video, 'error', { configurable: true, value: { code: 3 } }); window.__video.dispatchEvent(new Event('error'));`);
  await until(`document.querySelector('#playback-error')?.textContent.includes('could not read')`);
  assert.equal(await evaluate(`!!document.querySelector('.secondary-actions')`), true);
  await evaluate(`window.mountSample('second')`);
  await until(`document.querySelector('.media-player video')?.readyState >= 2`);
  assert.equal(await evaluate(`!document.querySelector('#playback-error') && document.querySelector('.media-player video') !== window.__video && document.querySelector('.media-player video').currentTime === 0`), true);
  await evaluate(`window.__posterData = null; window.mountSample('no-poster')`);
  await until(`document.querySelector('.media-player video')?.readyState >= 2`);
  assert.equal(await evaluate(`document.querySelector('.media-player video').poster === ''`), true);
  await evaluate(`window.__rejectedVideo = document.querySelector('.media-player video'); window.__rejectedVideo.play = () => Promise.reject(new DOMException('test', 'NotAllowedError')); document.querySelector('button[aria-label="Play"]').click()`);
  await until(`document.querySelector('#playback-error')?.textContent.includes('Playback could not start')`);
  await evaluate(`window.mountSample('fullscreen-failure')`);
  await until(`document.querySelector('.media-player video')?.readyState >= 2`);
  await evaluate(`document.querySelector('.media-player').requestFullscreen = () => Promise.reject(new TypeError('test')); document.querySelector('button[aria-label="Fullscreen"]').click()`);
  await until(`document.querySelector('.player-notice')?.textContent.includes('Fullscreen is unavailable')`);
  assert.equal(await evaluate(`document.fullscreenElement === null && document.querySelector('dialog').open`), true);
  await evaluate(`Object.defineProperty(document.querySelector('.media-player video'), 'duration', { configurable: true, value: Infinity }); document.querySelector('.media-player video').dispatchEvent(new Event('durationchange'))`);
  await until(`document.querySelector('.player-seek').disabled`);
  console.log('PASS rejected play/fullscreen promises and unknown duration');
  await evaluate(`window.mountSample('small-window')`);
  await until(`document.querySelector('.media-player video')?.readyState >= 2`);
  await send('Emulation.setDeviceMetricsOverride', { width: 980, height: 640, deviceScaleFactor: 1, mobile: false });
  await pause(100);
  assert.equal(await evaluate(`(() => { const root = document.querySelector('.media-player').getBoundingClientRect(); return [...document.querySelectorAll('.player-controls button, .player-controls input')].every(node => { const rect = node.getBoundingClientRect(); return rect.x >= root.x && rect.right <= root.right + 1 && rect.bottom <= root.bottom + 1; }); })()`), true);
  const small = await send('Page.captureScreenshot', { format: 'png' });
  await writeFile('node_modules/.tmp/player-redesign-small.png', Buffer.from(small.data, 'base64'));
  console.log('PASS controls stay inside player at minimum app window size');
  await evaluate(`document.querySelector('.media-player').focus()`);
  await key('k', 75);
  await until(`!document.querySelector('.media-player video').paused`);
  await evaluate(`window.__movingVideo = document.querySelector('.media-player video'); window.__movingTime = window.__movingVideo.currentTime; document.querySelector('button[aria-label="Theater Mode"]').click()`);
  await until(`document.querySelector('dialog').classList.contains('media-theater')`);
  assert.equal(await evaluate(`window.__movingVideo === document.querySelector('.media-player video') && !window.__movingVideo.paused && window.__movingVideo.currentTime >= window.__movingTime`), true);
  await evaluate(`document.querySelector('.media-player').focus()`);
  await key('f', 70);
  await until(`!!document.fullscreenElement`);
  await key('f', 70);
  await until(`!document.fullscreenElement`);
  assert.equal(await evaluate(`!window.__movingVideo.paused && window.__movingVideo === document.querySelector('.media-player video')`), true);
  await key('k', 75);
  await until(`window.__movingVideo.paused`);
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'k', windowsVirtualKeyCode: 75 });
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'k', windowsVirtualKeyCode: 75, autoRepeat: true });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'k', windowsVirtualKeyCode: 75 });
  assert.equal(await evaluate(`!window.__movingVideo.paused`), true);
  await key('k', 75);
  await until(`window.__movingVideo.paused`);
  const surface = await evaluate(`(() => { const r = document.querySelector('.player-surface').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  for (const clickCount of [1, 2]) {
    await send('Input.dispatchMouseEvent', { type: 'mousePressed', button: 'left', clickCount, ...surface });
    await send('Input.dispatchMouseEvent', { type: 'mouseReleased', button: 'left', clickCount, ...surface });
  }
  await until(`!!document.fullscreenElement`);
  await pause(450);
  assert.equal(await evaluate(`window.__movingVideo.paused`), true);
  await key('Escape', 27);
  await until(`!document.fullscreenElement`);
  console.log('PASS playing mode changes, F shortcut, key repeat protection, surface double-click');

  // Real decoded video fixtures; no library files or backend writes are involved.
  await evaluate(`(async () => {
    window.__audioSample = window.__sampleSrc;
    window.__ratioSamples = await Promise.all([[320,180],[240,180],[180,320]].map(async ([width,height]) => {
      const canvas = document.createElement('canvas'); canvas.width = width; canvas.height = height;
      const context = canvas.getContext('2d');
      context.fillStyle = '#384959'; context.fillRect(0,0,width,height);
      context.fillStyle = '#bdddfc'; context.fillRect(0,height-16,width,16);
      const poster = canvas.toDataURL(), stream = canvas.captureStream(10), chunks = [];
      const recorder = new MediaRecorder(stream, { mimeType:'video/webm;codecs=vp8' });
      recorder.ondataavailable = event => chunks.push(event.data);
      const stopped = new Promise(resolve => recorder.onstop = resolve);
      recorder.start();
      const paint = setInterval(() => { context.fillStyle = '#88bdf2'; context.fillRect(10,10,20,20); }, 80);
      await new Promise(resolve => setTimeout(resolve,1100));
      recorder.stop(); await stopped; clearInterval(paint); stream.getTracks().forEach(track => track.stop());
      return { src:URL.createObjectURL(new Blob(chunks,{type:'video/webm'})), poster, ratio:width/height };
    }));
  })()`);
  for (const [width, height] of [[1280,800],[980,640],[360,640]]) {
    await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor:1, mobile:false });
    const heights = new Map();
    for (const index of [0,1,2]) {
      await evaluate(`window.__sampleSrc = window.__ratioSamples[${index}].src; window.__posterData = window.__ratioSamples[${index}].poster; window.mountSample('ratio-${width}-${index}')`);
      await until(`document.querySelector('video')?.readyState >= 2`);
      for (const mode of ['normal','theater','fullscreen']) {
        if (mode === 'theater') {
          await evaluate(`document.querySelector('button[aria-label="Theater Mode"]').click()`);
          await until(`document.querySelector('dialog').classList.contains('media-theater')`);
          await until(`(() => { const root = document.querySelector('.media-player'), form = root.closest('form'), style = getComputedStyle(form); return root.querySelector('.player-stage').getBoundingClientRect().height > 0 && Math.abs(root.getBoundingClientRect().height - (form.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom))) <= 1; })()`);
        } else if (mode === 'fullscreen') {
          await evaluate(`document.querySelector('.media-player').focus()`);
          await key('f',70);
          await until(`document.querySelector('.media-player').matches(':fullscreen') && document.querySelector('.media-player').getBoundingClientRect().height === innerHeight`);
        }
        const geometry = await checkLayout(width + '/' + index + '/' + mode);
        assert.ok(Math.abs(geometry.ratio - [16/9,4/3,9/16][index]) < 0.001, 'fixture must have the intended decoded ratio');
        if (heights.has(mode)) assert.equal(geometry.controls.height, heights.get(mode), 'controls height must not depend on video resolution');
        else heights.set(mode, geometry.controls.height);
        if (width === 1280 && index === 2) {
          const shot = await send('Page.captureScreenshot', { format:'png' });
          await writeFile('node_modules/.tmp/player-redesign-' + mode + '.png', Buffer.from(shot.data,'base64'));
        }
        if (mode === 'fullscreen') {
          await key('Escape',27);
          await until(`!document.fullscreenElement && !document.querySelector('.media-player').matches(':fullscreen')`);
          assert.equal(await evaluate(`window.__closed === 0 && document.querySelector('dialog').open`), true);
        }
      }
    }
  }
  console.log('PASS 27 actual layout cases: 16:9 / 4:3 / 9:16, normal / theater / fullscreen, 1280 / 980 / 360 widths (CDP viewport emulation; not Windows DPI acceptance)');
  await evaluate(`window.__sampleSrc = window.__audioSample`);
  await evaluate(`window.mountSample('audio', 'audio')`);
  await until(`document.querySelector('audio')?.readyState >= 2`);
  assert.equal(await evaluate(`document.querySelector('audio').controls && !document.querySelector('.media-player')`), true);
  console.log('PASS decode error, preserved actions, media reset, no poster, native audio');
  assert.deepEqual(exceptions, []);
  console.log('PASS no uncaught browser exceptions');
} finally {
  ws.close();
}
