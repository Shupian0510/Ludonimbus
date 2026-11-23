// Injects the Ludonimbus cloud save status into the play bar of a game's
// library page, styled like Steam's own "Cloud status" item.
//
// Steam's class names are hashed, so they are looked up through Millennium's
// class modules (`playSectionClasses`). The page is a React app that re-renders
// often, so a MutationObserver puts the item back whenever it disappears.

import { playSectionClasses } from 'millennium';
import { describe, isGame, parseReply, Reply, t, View } from './status';

const MARK = 'data-ludonimbus';
const STYLE_ID = 'ludonimbus-style';
const POLL_MS = 5000;
const FAST_POLL_MS = 1500;
const REFRESH_MS = 2 * 60 * 1000;
const STEAMID64_BASE = BigInt('76561197960265728');

const CLOUD_SVG =
	'<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M19.35 10.04A7.49 7.49 0 0 0 12 4C9.11 4 6.6 5.64 5.35 8.04A5.994 5.994 0 0 0 0 14c0 3.31 2.69 6 6 6h13c2.76 0 5-2.24 5-5 0-2.64-2.05-4.78-4.65-4.96z"/></svg>';
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
					? el.querySelector(`:scope > [${MARK}]`)
					: el.nextElementSibling?.hasAttribute(MARK)
						? (el.nextElementSibling as HTMLElement)
						: null;
			if (!item) {
				item = this.create(mode);
				if (mode === 'stat') el.appendChild(item);
				else el.insertAdjacentElement('afterend', item);
			}
			this.fill(item, view);
		}
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
		item.addEventListener('click', () => this.openInApp());
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

	private openInApp(): void {
		if (isGame(this.reply)) backend.open_game(this.reply.key);
	}
}
