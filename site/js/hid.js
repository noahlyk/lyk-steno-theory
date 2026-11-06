// A steno machine over Plover HID (WebHID, so Chromium-based browsers only: Chrome, Edge, Brave).
// The machine sends the whole lever state as one 64-bit snapshot on every change; this decodes it
// to lyk key ids and feeds input.js, so every drill, the key stream and the Try it pad take the
// machine exactly as they take the keyboard. The protocol is Plover's (plover/machine/plover_hid.py)
// and the Uni v4's QMK keymap, as also read by the steno-lessons project.

import { feed } from './input.js';
import { store } from './store.js';

// The USB HID usage the firmware exposes; Plover's machine scans for exactly this pair.
export const USAGE_PAGE = 0xff50;
export const USAGE = 0x4c56;
export const REPORT_ID = 0x50;

// The 64 levers, most significant bit first. The Uni's 28 keys come first; the rest are for other
// boards on the protocol and never fire on a Uni.
export const CHART = [
  'S1-', 'T-', 'K-', 'P-', 'W-', 'H-', 'R-', 'A-',
  'O-', '*1', '-E', '-U', '-F', '-R', '-P', '-B',
  '-L', '-G', '-T', '-S', '-D', '-Z', '#1', 'S2-',
  '*2', '*3', '*4', '#2', '#3', '#4', '#5', '#6',
  '#7', '#8', '#9', '#A', '#B', '#C', 'X1', 'X2',
];

/**
 * The lyk key a lever presses, or null. The Uni splits S in two (S1- above S2-); lyk puts the join
 * key `+` on the top half of S, so by default the top one is `+` and the bottom one is `S-`.
 * Turn `topSIsJoin` off to have both press `S-`. All four asterisks are `*`; the `#` keys press
 * nothing (lyk has no number bar).
 */
export function leverKey(name, topSIsJoin = true) {
  if (name === 'S1-') return topSIsJoin ? '+' : 'S-';
  if (name === 'S2-') return 'S-';
  if (name.startsWith('*')) return '*';
  if (name === 'A-') return 'A';
  if (name === 'O-') return 'O';
  if (name === '-E') return 'E';
  if (name === '-U') return 'U';
  if (/^(#|X)/.test(name)) return null;
  return name;
}

/**
 * A Plover HID report as a Set of lyk key ids; null if it is not one. `reportId` and the payload
 * come apart in WebHID (the browser strips the id off the data), so both are taken separately.
 */
export function decodeReport(reportId, payload, topSIsJoin = true) {
  const bytes = payload instanceof DataView ? new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength) : payload;
  if (reportId !== REPORT_ID || !bytes || bytes.length < 8) return null;
  let state = 0n;
  for (let i = 0; i < 8; i += 1) state = (state << 8n) | BigInt(bytes[i] & 0xff);
  const keys = new Set();
  for (let i = 0; i < CHART.length; i += 1) {
    if ((state >> BigInt(63 - i)) & 1n) {
      const key = leverKey(CHART[i], topSIsJoin);
      if (key) keys.add(key);
    }
  }
  return keys;
}

export const supported = () => typeof navigator !== 'undefined' && 'hid' in navigator;

const isMachine = (d) => d.collections?.some((c) => c.usagePage === USAGE_PAGE && c.usage === USAGE);

let device = null;
let onStatus = () => {};

const topSIsJoin = () => store.settings.topSIsJoin !== false;

function onReport(e) {
  const keys = decodeReport(e.reportId, e.data, topSIsJoin());
  if (keys) feed(keys);
}

async function attach(d) {
  if (device || !d) return false;
  try {
    if (!d.opened) await d.open();
    d.addEventListener('inputreport', onReport);
    device = d;
    onStatus({ connected: true, name: d.productName ?? 'steno machine' });
    return true;
  } catch (error) {
    onStatus({ connected: false, error: error instanceof Error ? error.message : String(error) });
    return false;
  }
}

/** Opens the browser's device picker (needs a click). */
export async function connect() {
  if (!supported()) return false;
  try {
    const picked = await navigator.hid.requestDevice({ filters: [{ usagePage: USAGE_PAGE, usage: USAGE }] });
    return attach(picked.find(isMachine) ?? picked[0]);
  } catch (error) {
    onStatus({ connected: false, error: error instanceof Error ? error.message : String(error) });
    return false;
  }
}

/** Reconnects a machine this site was already allowed to use, without a prompt. */
export async function autoConnect() {
  if (!supported()) return false;
  try {
    return attach((await navigator.hid.getDevices()).find(isMachine));
  } catch {
    return false;
  }
}

export async function disconnect() {
  const d = device;
  device = null;
  if (!d) return;
  d.removeEventListener('inputreport', onReport);
  try {
    await d.close();
  } catch {
    // already gone
  }
  feed(new Set());
  onStatus({ connected: false });
}

/** Wires the header button and plug/unplug events. `host` is the element the button goes in. */
export function initHid(host) {
  if (!supported()) return;
  const label = document.createElement('span');
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'btn hid__btn';
  button.textContent = 'Connect steno machine';
  const wrap = document.createElement('div');
  wrap.className = 'hid';
  const join = document.createElement('input');
  join.type = 'checkbox';
  join.checked = topSIsJoin();
  join.addEventListener('change', () => store.setSetting('topSIsJoin', join.checked));
  const joinLabel = document.createElement('label');
  joinLabel.className = 'hid__join';
  joinLabel.title = 'The Uni splits S in two; lyk puts the join key on the top half';
  joinLabel.append(join, ' top S is the join key');
  wrap.append(button, label, joinLabel);
  host.append(wrap);
  onStatus = (s) => {
    button.textContent = s.connected ? 'Disconnect machine' : 'Connect steno machine';
    label.textContent = s.connected ? `${s.name} connected` : s.error ? `not connected: ${s.error}` : '';
    wrap.classList.toggle('is-on', Boolean(s.connected));
  };
  button.addEventListener('click', () => (device ? disconnect() : connect()));
  navigator.hid.addEventListener('disconnect', (e) => { if (e.device === device) disconnect(); });
  navigator.hid.addEventListener('connect', (e) => { if (isMachine(e.device)) attach(e.device); });
  autoConnect();
}
