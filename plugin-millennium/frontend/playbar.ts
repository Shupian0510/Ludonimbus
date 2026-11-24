// Injects the Ludonimbus cloud save status into the play bar of a game's
// library page, styled like Steam's own "Cloud status" item.
//
// Steam's class names are hashed, so they are looked up through Millennium's
// class modules (`playSectionClasses`). The page is a React app that re-renders
// often, so a MutationObserver puts the item back whenever it disappears.

import { playSectionClasses } from 'millennium';
import { describe, isGame, parseReply, playtimeView, Reply, t, View } from './status';
import { showVersions } from './versions';

const MARK = 'data-ludonimbus';
const STYLE_ID = 'ludonimbus-style';
const POLL_MS = 5000;
const FAST_POLL_MS = 1500;
const REFRESH_MS = 2 * 60 * 1000;
const STEAMID64_BASE = BigInt('76561197960265728');

const CLOUD_SVG =
	'<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M19.35 10.04A7.49 7.49 0 0 0 12 4C9.11 4 6.6 5.64 5.35 8.04A5.994 5.994 0 0 0 0 14c0 3.31 2.69 6 6 6h13c2.76 0 5-2.24 5-5 0-2.64-2.05-4.78-4.65-4.96z"/></svg>';
const CLOCK_SVG =
	'<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zm0 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16zm.5-13H11v6l5.2 3.2.8-1.3-4.5-2.7z"/></svg>';
const SYNC_SVG =
	'<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M12 4V1L8 5l4 4V6c3.31 0 6 2.69 6 6 0 1.01-.25 1.97-.7 2.8l1.46 1.46A7.93 7.93 0 0 0 20 12c0-4.42-3.58-8-8-8zm0 14c-3.31 0-6-2.69-6-6 0-1.01.25-1.97.7-2.8L5.24 7.74A7.93 7.93 0 0 0 4 12c0 4.42 3.58 8 8 8v3l4-4-4-4v3z"/></svg>';

// Typography comes from Steam's own classes (PlayBarLabel, PlayBarDetailLabel,
// GameStatIcon); `.ludonimbus-plain` only applies when they were not found.
const CSS = `
.ludonimbus-stat { display: flex; align-items: center; cursor: pointer; min-width: 0; }
.ludonimbus-stat .ludonimbus-icon { display: flex; flex: none; }
.ludonimbus-stat .ludonimbus-icon svg { width: 26px; height: 26px; }
.ludonimbus-stat .ludonimbus-text { min-width: 0; }
.ludonimbus-stat .ludonimbus-detail { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 340px; }
.ludonimbus-stat[data-busy="1"] .ludonimbus-icon { animation: ludonimbus-pulse 1.2s ease-in-out infinite; color: #66c0f4; }
.ludonimbus-stat[data-tone="info"] .ludonimbus-detail { color: rgba(102, 192, 244, 0.9); }
.ludonimbus-stat[data-tone="warn"] .ludonimbus-detail { color: #e5b143; }
.ludonimbus-stat[data-tone="bad"] .ludonimbus-detail { color: #ff7b7b; }
.ludonimbus-stat:hover .ludonimbus-label { color: rgba(255, 255, 255, 0.8); }
.ludonimbus-stat .ludonimbus-sync { display: none; flex: none; align-items: center; justify-content: center; width: 26px; height: 26px; margin-left: 8px; padding: 0; border: 0; border-radius: 3px; background: rgba(255,255,255,0.06); color: rgba(255,255,255,0.6); cursor: pointer; }
.ludonimbus-stat .ludonimbus-sync svg { width: 16px; height: 16px; }
.ludonimbus-stat[data-can-sync="1"] .ludonimbus-sync { display: flex; }
.ludonimbus-stat .ludonimbus-sync:hover { background: rgba(255,255,255,0.16); color: #fff; }
.ludonimbus-plain { gap: 6px; }
.ludonimbus-plain .ludonimbus-icon { margin: 3px; color: rgba(255, 255, 255, 0.4); }
.ludonimbus-plain .ludonimbus-label { font-size: 14px; text-transform: uppercase; color: rgba(255, 255, 255, 0.52); white-space: nowrap; }
.ludonimbus-plain .ludonimbus-detail { font-size: 13px; color: rgba(255, 255, 255, 0.32); }
.ludonimbus-bar { padding: 8px 16px; background: rgba(0,0,0,0.2); }
.ludonimbus-playtime { cursor: default; }
.ludonimbus-versions-modal { min-width: 560px; }
.ludonimbus-versions { display: flex; flex-direction: column; gap: 6px; max-height: 52vh; overflow-y: auto; padding-right: 4px; }
.ludonimbus-version { display: flex; align-items: center; gap: 12px; padding: 10px 12px; border-radius: 3px; background: rgba(255,255,255,0.04); }
.ludonimbus-version[data-current="1"] { background: rgba(102,192,244,0.10); }
.ludonimbus-version-main { flex: 1; min-width: 0; }
.ludonimbus-version-title { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; color: #fff; font-size: 14px; }
.ludonimbus-version-device { color: rgba(255,255,255,0.6); }
.ludonimbus-version-sub { margin-top: 3px; font-size: 12px; color: rgba(255,255,255,0.45); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ludonimbus-version-restore { width: auto !important; min-width: 90px; flex: none; }
.ludonimbus-version-confirm { margin-top: 8px; font-size: 13px; color: rgba(255,255,255,0.75); }
.ludonimbus-version-actions { display: flex; gap: 8px; margin-top: 8px; }
.ludonimbus-version-actions > * { width: auto !important; min-width: 100px; }
.ludonimbus-tag { font-size: 11px; padding: 1px 6px; border-radius: 2px; background: rgba(255,255,255,0.1); color: rgba(255,255,255,0.7); text-transform: uppercase; }
.ludonimbus-tag-blue { background: rgba(102,192,244,0.2); color: #66c0f4; }
.ludonimbus-versions-footer { display: flex; gap: 8px; justify-content: flex-end; }
.ludonimbus-versions-footer > * { width: auto !important; min-width: 120px; }
.ludonimbus-muted { color: rgba(255,255,255,0.5); font-size: 13px; margin: 6px 0; }
.ludonimbus-error { color: #ff7b7b; font-size: 13px; margin: 6px 0; }
.ludonimbus-message { margin-top: 10px; }
@keyframes ludonimbus-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.35; } }
`;

function cls(name: string): string {
	return ((playSectionClasses as Record<string, string> | undefined)?.[name] ?? '').trim();
}

function currentAppId(): string | null {
	const path: string = (window as any).MainWindowBrowserManager?.m_lastLocation?.pathname ?? '';
	const m = /\/library\/app\/(\d+)/.exec(path);
	return m ? m[1] : null;
}

/** Steam account id (the `userdata` folder name) of the signed-in user. */
function currentAccount(): string {
	try {
		const id: string | undefined = (window as any).App?.m_CurrentUser?.strSteamID;
		return id ? (BigInt(id) - STEAMID64_BASE).toString() : '';
	} catch {
		return '';
	}
}

export class PlayBarStatus {
	private appId: string | null = null;
	private reply: Reply | null = null;
	private observer: MutationObserver | null = null;
	private timer: number | undefined;
	private fast = false;
	private scheduled = false;
	private lastRefresh = 0;
	private lastPhase: string | null = null;

	constructor(private readonly doc: Document) {}

	start(): void {
		this.injectStyle();
		this.observer = new MutationObserver(() => this.schedule());
		this.observer.observe(this.doc.body, { childList: true, subtree: true });
		this.schedule();
		this.poll();
	}

	stop(): void {
		this.observer?.disconnect();
		this.observer = null;
		window.clearTimeout(this.timer);
		this.timer = undefined;
		this.removeAll();
		this.doc.getElementById(STYLE_ID)?.remove();
	}

	private injectStyle(): void {
		if (this.doc.getElementById(STYLE_ID)) return;
		const style = this.doc.createElement('style');
		style.id = STYLE_ID;
		style.textContent = CSS;
		this.doc.head.appendChild(style);
	}

	/** Coalesce bursts of DOM mutations into one update. */
	private schedule(): void {
		if (this.scheduled) return;
		this.scheduled = true;
		window.setTimeout(() => {
			this.scheduled = false;
			this.update();
		}, 150);
	}

	private update(): void {
		const appId = currentAppId();
		if (appId !== this.appId) {
			this.appId = appId;
			this.reply = null;
			this.lastPhase = null;
			this.removeAll();
			if (appId) {
				// Cached status first (instant), then a fresh check.
				this.load(appId, false).then(() => this.load(appId, true));
			}
			return;
		}
		this.render();
	}

	private poll(): void {
		window.clearTimeout(this.timer);
		this.timer = undefined;
		const appId = this.appId;
		const run = async () => {
			if (appId && appId === this.appId && !this.doc.hidden) {
				const refresh = Date.now() - this.lastRefresh > REFRESH_MS;
				await this.load(appId, refresh);
			}
		};
		run().finally(() => {
			if (this.observer && this.timer === undefined) this.later(this.busy() ? FAST_POLL_MS : POLL_MS);
		});
	}

	private later(ms: number): void {
		window.clearTimeout(this.timer);
		this.fast = ms === FAST_POLL_MS;
		this.timer = window.setTimeout(() => this.poll(), ms);
	}

	private busy(): boolean {
		return isGame(this.reply) && this.reply.phase !== 'idle';
	}

	private async load(appId: string, refresh: boolean): Promise<void> {
		if (refresh) this.lastRefresh = Date.now();
		let reply: Reply;
		try {
			reply = parseReply(await backend.get_status(appId, currentAccount(), refresh));
		} catch (e) {
			reply = { error: 'backend', detail: String(e) };
		}
		if (appId !== this.appId) return;
		const phase = isGame(reply) ? reply.phase : null;
		const finished = this.lastPhase !== null && this.lastPhase !== 'idle' && phase === 'idle';
		this.lastPhase = phase;
		this.reply = reply;
		this.render();
		// Something is in progress: follow it closely.
		if (this.busy() && !this.fast && this.observer) this.later(FAST_POLL_MS);
		// A sync just ended somewhere (Steam wrapper, tray watcher): re-check.
		if (finished && !refresh) await this.load(appId, true);
	}

	private removeAll(): void {
		this.doc.querySelectorAll(`[${MARK}]`).forEach((el) => el.remove());
	}

	/** Where the item goes: Steam's play bar stats, or after the play bar. */
	private targets(): { el: Element; mode: 'stat' | 'bar' }[] {
		const stats = cls('GameStatsSection');
		if (stats) {
			const found = Array.from(this.doc.querySelectorAll(`.${stats.split(/\s+/)[0]}`));
			if (found.length) return found.map((el) => ({ el, mode: 'stat' as const }));
		}
		const bar = cls('PlayBar');
		const playBar = bar ? this.doc.querySelector(`.${bar.split(/\s+/)[0]}`) : null;
		return playBar ? [{ el: playBar, mode: 'bar' as const }] : [];
	}

	private render(): void {
		const view = this.appId ? describe(this.reply, this.appId) : null;
		if (!view) {
			this.removeAll();
			return;
		}
		for (const { el, mode } of this.targets()) {
			let item: HTMLElement | null =
				mode === 'stat'
					? el.querySelector(`:scope > [${MARK}="stat"]`)
					: el.nextElementSibling?.hasAttribute(MARK)
						? (el.nextElementSibling as HTMLElement)
						: null;
			if (!item) {
				item = this.create(mode);
				if (mode === 'stat') el.appendChild(item);
				else el.insertAdjacentElement('afterend', item);
			}
			this.fill(item, view);
			if (mode === 'stat') this.renderPlaytime(el, item);
		}
	}

	/** "All devices" playtime next to Steam's own (this PC only) playtime. */
	private renderPlaytime(stats: Element, status: HTMLElement): void {
		const pt = playtimeView(this.reply);
		let item = stats.querySelector<HTMLElement>(`:scope > [${MARK}="playtime"]`);
		if (!pt) {
			item?.remove();
			return;
		}
		if (!item) {
			const d = this.doc;
			item = d.createElement('div');
			item.setAttribute(MARK, 'playtime');
			const native = !!(cls('GameStat') && cls('PlayBarLabel') && cls('PlayBarDetailLabel'));
			item.className = ['ludonimbus-stat', 'ludonimbus-playtime', native ? '' : 'ludonimbus-plain', cls('GameStat')]
				.filter(Boolean)
				.join(' ');
			const icon = d.createElement('div');
			icon.className = ['ludonimbus-icon', cls('GameStatIcon')].filter(Boolean).join(' ');
			icon.innerHTML = CLOCK_SVG;
			const text = d.createElement('div');
			text.className = ['ludonimbus-text', cls('GameStatRight')].filter(Boolean).join(' ');
			const label = d.createElement('div');
			label.className = ['ludonimbus-label', cls('PlayBarLabel')].filter(Boolean).join(' ');
			const detail = d.createElement('div');
			detail.className = ['ludonimbus-detail', cls('PlayBarDetailLabel')].filter(Boolean).join(' ');
			text.append(label, detail);
			item.append(icon, text);
			stats.insertBefore(item, status);
		}
		const label = item.querySelector('.ludonimbus-label');
		if (label && label.textContent !== t().allDevices) label.textContent = t().allDevices;
		const detail = item.querySelector('.ludonimbus-detail');
		if (detail && detail.textContent !== pt.text) detail.textContent = pt.text;
		if (item.title !== pt.tip) item.title = pt.tip;
	}

	private create(mode: 'stat' | 'bar'): HTMLElement {
		const d = this.doc;
		const wrap = d.createElement('div');
		wrap.setAttribute(MARK, mode);
		const item = mode === 'bar' ? d.createElement('div') : wrap;
		if (mode === 'bar') {
			wrap.className = 'ludonimbus-bar';
			wrap.appendChild(item);
		}
		const native = !!(cls('GameStat') && cls('PlayBarLabel') && cls('PlayBarDetailLabel'));
		item.className = ['ludonimbus-stat', native ? '' : 'ludonimbus-plain', cls('GameStat'), cls('ClickablePlayBarItem')]
			.filter(Boolean)
			.join(' ');

		const icon = d.createElement('div');
		icon.className = ['ludonimbus-icon', cls('GameStatIcon')].filter(Boolean).join(' ');
		icon.innerHTML = CLOUD_SVG;

		const text = d.createElement('div');
		text.className = ['ludonimbus-text', cls('GameStatRight')].filter(Boolean).join(' ');
		const label = d.createElement('div');
		label.className = ['ludonimbus-label', cls('PlayBarLabel')].filter(Boolean).join(' ');
		label.textContent = t().label;
		const detail = d.createElement('div');
		detail.className = ['ludonimbus-detail', cls('PlayBarDetailLabel')].filter(Boolean).join(' ');
		text.append(label, detail);

		const sync = d.createElement('button');
		sync.className = 'ludonimbus-sync';
		sync.title = t().syncNow;
		sync.innerHTML = SYNC_SVG;
		sync.addEventListener('click', (e) => {
			e.stopPropagation();
			this.syncNow();
		});

		item.append(icon, text, sync);
		item.addEventListener('click', () => this.click());
		return wrap;
	}

	private fill(wrap: HTMLElement, view: View): void {
		const item = wrap.classList.contains('ludonimbus-stat') ? wrap : (wrap.firstElementChild as HTMLElement);
		const detail = item.querySelector('.ludonimbus-detail');
		if (detail && detail.textContent !== view.text) detail.textContent = view.text;
		// The language may only be known after the first reply.
		const label = item.querySelector('.ludonimbus-label');
		if (label && label.textContent !== t().label) label.textContent = t().label;
		const sync = item.querySelector<HTMLElement>('.ludonimbus-sync');
		if (sync && sync.title !== t().syncNow) sync.title = t().syncNow;
		if (item.title !== view.tip) item.title = view.tip;
		item.dataset.tone = view.tone;
		item.dataset.busy = view.busy ? '1' : '0';
		item.dataset.canSync = view.canSync ? '1' : '0';
	}

	private async syncNow(): Promise<void> {
		if (!isGame(this.reply)) return;
		const appId = this.appId;
		const reply = parseReply(await backend.sync_now(this.reply.key));
		if ('error' in reply) {
			const detail = reply.detail ?? reply.error;
			this.doc.querySelectorAll(`[${MARK}] .ludonimbus-detail`).forEach((el) => (el.textContent = detail));
			return;
		}
		if (appId) await this.load(appId, false);
	}

	/** Show the version history, or open Ludonimbus for what only it can fix. */
	private click(): void {
		const reply = this.reply;
		if (!isGame(reply) || !this.appId) return;
		const view = describe(reply, this.appId);
		if (view?.action === 'versions') {
			const appId = this.appId;
			showVersions(this.doc.defaultView, reply.key, reply.name, () => {
				// A sync or restore started: follow it closely.
				this.load(appId, false).then(() => this.later(FAST_POLL_MS));
			});
		} else {
			backend.open_game(reply.key);
		}
	}
}
