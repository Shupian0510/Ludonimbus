// Achievements on a game's library page, where Steam shows them for its own
// games:
//   - the play bar stat "Achievements 12/40" with a progress bar (Steam's
//     `GameStat MiniAchievements` markup and classes), before the cloud item;
//   - an "Achievements" section at the top of the right column (Steam's
//     `AppDetailsSection` / `RightColumnSection` / `SectionHeader` classes),
//     with progress, unlocked and locked icons and "View all".
// Both open the full list. Only games with an achievement source linked in
// Ludonimbus show them. Class names are hashed and looked up at run time.

import { findClassModule } from 'millennium';
import { cls, currentAccount, currentAppId } from './playbar';

const MARK = 'data-ludonimbus-ach';
const STYLE_ID = 'ludonimbus-ach-style';
const POLL_MS = 20000;
export const UNLOCK_EVENT = 'ludonimbus-achievement';

const TROPHY_SVG =
	'<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M19 4h-2V3H7v1H5a2 2 0 0 0-2 2v1a5 5 0 0 0 4.4 4.96A5.01 5.01 0 0 0 11 15.9V18H8v2h8v-2h-3v-2.1a5.01 5.01 0 0 0 3.6-3.94A5 5 0 0 0 21 7V6a2 2 0 0 0-2-2zM5 7V6h2v3.83A3 3 0 0 1 5 7zm14 0a3 3 0 0 1-2 2.83V6h2v1z"/></svg>';

const CSS = `
.ludonimbus-ach { display: flex; align-items: center; cursor: pointer; min-width: 0; }
.ludonimbus-ach .ludonimbus-icon { display: flex; flex: none; }
.ludonimbus-ach .ludonimbus-icon svg { width: 26px; height: 26px; }
.ludonimbus-ach.ludonimbus-plain { gap: 6px; }
.ludonimbus-ach.ludonimbus-plain .ludonimbus-icon { margin: 3px; color: rgba(255,255,255,0.4); }
.ludonimbus-ach.ludonimbus-plain .ludonimbus-label { font-size: 14px; text-transform: uppercase; color: rgba(255,255,255,0.52); }
.ludonimbus-ach.ludonimbus-plain .ludonimbus-detail { font-size: 13px; color: rgba(255,255,255,0.32); }
.ludonimbus-ach-plainbar { height: 3px; margin-top: 4px; width: 64px; background: rgba(255,255,255,0.12); border-radius: 2px; overflow: hidden; }
.ludonimbus-ach-row-inline { display: flex; align-items: center; gap: 8px; }
.ludonimbus-ach-section .ludonimbus-ach-sec-body { padding: 12px 16px 14px; cursor: pointer; }
.ludonimbus-ach-section .ludonimbus-ach-sec-body:hover { background: rgba(255,255,255,0.04); }
.ludonimbus-ach-sec-plainhead { font-size: 14px; text-transform: uppercase; letter-spacing: 0.04em; color: rgba(255,255,255,0.8); padding: 0 0 8px 2px; }
.ludonimbus-ach-sec-summary { font-size: 13px; color: #c6d4df; }
.ludonimbus-ach-sec-progress { height: 6px; margin: 8px 0 12px; background: rgba(255,255,255,0.1); border-radius: 3px; overflow: hidden; }
.ludonimbus-ach-sec-progress > div { height: 100%; background: linear-gradient(90deg, #1a9fff, #66c0f4); }
.ludonimbus-ach-sec-icons { display: grid; grid-template-columns: repeat(auto-fill, minmax(40px, 1fr)); gap: 6px; }
.ludonimbus-ach-sec-icons > img, .ludonimbus-ach-sec-icons > div { width: 100%; aspect-ratio: 1; border-radius: 3px; object-fit: cover; background: rgba(255,255,255,0.06); }
.ludonimbus-ach-sec-icons > img[data-locked="1"] { filter: grayscale(1) brightness(0.45); }
.ludonimbus-ach-sec-icons > .ludonimbus-ach-sec-rest { display: flex; align-items: center; justify-content: center; font-size: 12px; color: #8f98a0; }
.ludonimbus-ach-sec-all { margin-top: 12px; font-size: 12px; color: #66c0f4; text-align: right; }
.ludonimbus-ach-backdrop { position: fixed; inset: 0; z-index: 9999; background: rgba(0,0,0,0.6); display: flex; align-items: center; justify-content: center; }
.ludonimbus-ach-panel { width: min(760px, 92vw); max-height: 84vh; display: flex; flex-direction: column; background: #1b2838; color: #dcdedf; border-radius: 4px; box-shadow: 0 12px 48px rgba(0,0,0,0.6); font-family: "Motiva Sans", Arial, sans-serif; }
.ludonimbus-ach-head { padding: 18px 22px 12px; border-bottom: 1px solid rgba(255,255,255,0.08); }
.ludonimbus-ach-title { font-size: 20px; color: #fff; display: flex; align-items: center; gap: 10px; }
.ludonimbus-ach-title .ludonimbus-ach-close { margin-left: auto; background: none; border: 0; color: #8f98a0; font-size: 22px; cursor: pointer; }
.ludonimbus-ach-title .ludonimbus-ach-close:hover { color: #fff; }
.ludonimbus-ach-sub { margin-top: 6px; font-size: 13px; color: #8f98a0; }
.ludonimbus-ach-progress { margin-top: 10px; height: 6px; background: rgba(255,255,255,0.1); border-radius: 3px; overflow: hidden; }
.ludonimbus-ach-progress > div { height: 100%; background: linear-gradient(90deg, #1a9fff, #66c0f4); }
.ludonimbus-ach-list { overflow-y: auto; padding: 8px 14px 16px; }
.ludonimbus-ach-row { display: flex; align-items: center; gap: 14px; padding: 8px; border-radius: 3px; }
.ludonimbus-ach-row:nth-child(odd) { background: rgba(0,0,0,0.15); }
.ludonimbus-ach-row img, .ludonimbus-ach-row .ludonimbus-ach-noicon { width: 56px; height: 56px; flex: none; border-radius: 3px; object-fit: cover; background: rgba(255,255,255,0.06); }
.ludonimbus-ach-row[data-locked="1"] img { filter: grayscale(1) brightness(0.55); }
.ludonimbus-ach-noicon { display: flex; align-items: center; justify-content: center; color: rgba(255,255,255,0.3); }
.ludonimbus-ach-noicon svg { width: 55%; height: 55%; }
.ludonimbus-ach-row[data-locked="1"] .ludonimbus-ach-name { color: #8f98a0; }
.ludonimbus-ach-text { flex: 1; min-width: 0; }
.ludonimbus-ach-name { color: #fff; font-size: 15px; }
.ludonimbus-ach-desc { color: #8f98a0; font-size: 13px; margin-top: 2px; }
.ludonimbus-ach-meta { flex: none; text-align: right; font-size: 12px; color: #8f98a0; white-space: nowrap; }
.ludonimbus-ach-tag { display: inline-block; margin-left: 6px; padding: 0 5px; border-radius: 2px; font-size: 11px; background: rgba(255,255,255,0.1); color: #c6d4df; }
.ludonimbus-ach-tag[data-kind="platinum"] { background: #6c8ead; color: #fff; }
.ludonimbus-ach-tag[data-kind="gold"] { background: #b8912b; color: #fff; }
.ludonimbus-ach-tag[data-kind="silver"] { background: #8a8f98; color: #fff; }
.ludonimbus-ach-tag[data-kind="bronze"] { background: #8c5a33; color: #fff; }
`;

type Lang = 'zh-CN' | 'en';
let lang: Lang = (navigator.language || '').toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';

const TEXTS = {
	'zh-CN': {
		label: '成就',
		detail: (u: number, n: number) => `${u} / ${n}`,
		tip: '点击查看全部成就',
		unlockedOf: (u: number, n: number, pct: number) => `已解锁 ${u} / ${n}（${pct}%）`,
		points: (p: number, total: number) => ` · ${p} / ${total} 分`,
		from: (s: string) => ` · 来源：${s}`,
		hidden: '隐藏成就',
		hiddenDesc: '继续游戏以解锁',
		on: (when: string, device: string | null) => `${when}${device ? `\n${device}` : ''}`,
		hardcore: '硬核',
		grades: { platinum: '白金', gold: '金', silver: '银', bronze: '铜' } as Record<string, string>,
		pts: (p: number) => `${p} 分`,
		close: '关闭',
		summary: (u: number, n: number, pct: number) => `您已解锁 ${u}/${n}（${pct}%）`,
		viewAll: '查看所有成就',
	},
	en: {
		label: 'Achievements',
		detail: (u: number, n: number) => `${u} / ${n}`,
		tip: 'Click to see all achievements',
		unlockedOf: (u: number, n: number, pct: number) => `${u} of ${n} unlocked (${pct}%)`,
		points: (p: number, total: number) => ` · ${p} / ${total} points`,
		from: (s: string) => ` · from ${s}`,
		hidden: 'Hidden achievement',
		hiddenDesc: 'Keep playing to unlock it',
		on: (when: string, device: string | null) => `${when}${device ? `\n${device}` : ''}`,
		hardcore: 'Hardcore',
		grades: { platinum: 'Platinum', gold: 'Gold', silver: 'Silver', bronze: 'Bronze' } as Record<string, string>,
		pts: (p: number) => `${p} pts`,
		close: 'Close',
		summary: (u: number, n: number, pct: number) => `You've unlocked ${u}/${n} (${pct}%)`,
		viewAll: 'View all achievements',
	},
};

const tx = () => TEXTS[lang];

export interface AchievementItem {
	id: string;
	name: string;
	description: string;
	iconUrl: string | null;
	hasIcon: boolean;
	hidden: boolean;
	points: number | null;
	grade: string | null;
	unlockedAt: string | null;
	hardcore: boolean;
	deviceName: string | null;
}

export interface AchievementsReply {
	managed: boolean;
	key?: string;
	name?: string;
	source?: string | null;
	sourceLabel?: string | null;
	title?: string | null;
	total?: number;
	unlocked?: number;
	points?: number;
	pointsTotal?: number;
	items?: AchievementItem[];
	/** Some achievements have no icon: show the game's (get_game_icon). */
	gameIcon?: boolean;
	lang?: string;
	error?: string;
}

function parse(value: unknown): AchievementsReply {
	let v = value;
	if (typeof v === 'string') {
		try {
			v = JSON.parse(v);
		} catch {
			return { managed: false, error: 'bad_reply' };
		}
	}
	return (v ?? { managed: false }) as AchievementsReply;
}

type Shown = AchievementsReply & { key: string; total: number; unlocked: number };

/** Whether a reply should show the play bar item and the section. */
export function shows(r: AchievementsReply | null): r is Shown {
	return !!r && r.managed && !!r.source && typeof r.key === 'string' && (r.total ?? 0) > 0;
}

function when(iso: string): string {
	const d = new Date(iso);
	if (Number.isNaN(d.getTime())) return '';
	return d.toLocaleDateString(lang === 'zh-CN' ? 'zh-CN' : undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

const icons = new Map<string, string>();

async function localIcon(key: string, id: string): Promise<string | null> {
	const k = `${key}/${id}`;
	if (icons.has(k)) return icons.get(k)!;
	try {
		const v = parse(await backend.get_achievement_icon(key, id)) as unknown as { dataUrl?: string };
		if (v.dataUrl) {
			icons.set(k, v.dataUrl);
			return v.dataUrl;
		}
	} catch {
		/* no icon */
	}
	return null;
}

const gameIcons = new Map<string, Promise<string | null>>();

/** The game's icon, for achievements without one (fetched once per game). */
function gameIcon(key: string): Promise<string | null> {
	let p = gameIcons.get(key);
	if (!p) {
		p = (async () => {
			try {
				const v = parse(await backend.get_game_icon(key)) as unknown as { dataUrl?: string };
				return v.dataUrl ?? null;
			} catch {
				return null;
			}
		})();
		gameIcons.set(key, p);
		// Not remembered when missing: an icon set later shows on the next look.
		p.then((u) => u || gameIcons.delete(key));
	}
	return p;
}

export class AchievementsBar {
	private appId: string | null = null;
	private reply: AchievementsReply | null = null;
	private observer: MutationObserver | null = null;
	private timer: number | undefined;
	private scheduled = false;
	private readonly onUnlock = () => this.appId && this.load(this.appId);

	constructor(private readonly doc: Document) {}

	start(): void {
		if (!this.doc.getElementById(STYLE_ID)) {
			const style = this.doc.createElement('style');
			style.id = STYLE_ID;
			style.textContent = CSS;
			this.doc.head.appendChild(style);
		}
		this.observer = new MutationObserver(() => this.schedule());
		this.observer.observe(this.doc.body, { childList: true, subtree: true });
		window.addEventListener(UNLOCK_EVENT, this.onUnlock);
		this.schedule();
		this.poll();
	}

	stop(): void {
		this.observer?.disconnect();
		this.observer = null;
		window.removeEventListener(UNLOCK_EVENT, this.onUnlock);
		window.clearTimeout(this.timer);
		this.doc.querySelectorAll(`[${MARK}]`).forEach((el) => el.remove());
		this.doc.getElementById(STYLE_ID)?.remove();
	}

	private schedule(): void {
		if (this.scheduled) return;
		this.scheduled = true;
		window.setTimeout(() => {
			this.scheduled = false;
			const appId = currentAppId();
			if (appId !== this.appId) {
				this.appId = appId;
				this.reply = null;
				this.doc.querySelectorAll(`[${MARK}]`).forEach((el) => el.remove());
				if (appId) this.load(appId);
				return;
			}
			this.render();
		}, 150);
	}

	private poll(): void {
		window.clearTimeout(this.timer);
		if (this.appId && !this.doc.hidden) this.load(this.appId);
		if (this.observer) this.timer = window.setTimeout(() => this.poll(), POLL_MS);
	}

	private async load(appId: string): Promise<void> {
		let reply: AchievementsReply;
		try {
			reply = parse(await backend.get_achievements(appId, currentAccount()));
		} catch {
			reply = { managed: false };
		}
		if (appId !== this.appId) return;
		if (reply.lang === 'zh-CN' || reply.lang === 'en') lang = reply.lang;
		this.reply = reply;
		this.render();
	}

	private render(): void {
		const r = this.reply;
		if (!shows(r)) {
			this.doc.querySelectorAll(`[${MARK}]:not([${MARK}="list"])`).forEach((el) => el.remove());
			return;
		}
		this.renderStats(r);
		this.renderSection(r);
	}

	/** Steam's play bar stat, in every stats row (Steam keeps more than one). */
	private renderStats(r: Shown): void {
		const stats = cls('GameStatsSection').split(/\s+/)[0];
		if (!stats) return;
		const m = tx();
		const pct = Math.floor((100 * r.unlocked) / Math.max(1, r.total));
		for (const section of Array.from(this.doc.querySelectorAll(`.${stats}`))) {
			let item = section.querySelector<HTMLElement>(`:scope > [${MARK}="stat"]`);
			if (!item) item = this.createStat();
			// Ludonimbus's items follow Steam's, most important first: cloud saves, then
			// achievements. The row wraps and the second line is not shown, so whatever
			// doesn't fit disappears from the end.
			const cloud = section.querySelector(':scope > [data-ludonimbus="stat"]');
			if (cloud) {
				if (cloud.nextElementSibling !== item) cloud.after(item);
			} else if (item.parentElement !== section) {
				section.appendChild(item);
			}
			const label = item.querySelector('.ludonimbus-label');
			if (label && label.textContent !== m.label) label.textContent = m.label;
			const count = item.querySelector('.ludonimbus-detail');
			const text = `${r.unlocked}/${r.total}`;
			if (count && count.textContent !== text) count.textContent = text;
			const fill = item.querySelector<HTMLElement>('.ludonimbus-fill');
			if (fill && fill.style.width !== `${pct}%`) fill.style.width = `${pct}%`;
			if (item.title !== m.tip) item.title = m.tip;
		}
	}

	private createStat(): HTMLElement {
		const d = this.doc;
		const native = !!(cls('GameStat') && cls('PlayBarLabel') && cls('PlayBarDetailLabel'));
		const item = d.createElement('div');
		item.setAttribute(MARK, 'stat');
		item.className = ['ludonimbus-ach', native ? '' : 'ludonimbus-plain', cls('GameStat'), cls('MiniAchievements'), cls('ClickablePlayBarItem')]
			.filter(Boolean)
			.join(' ');
		const icon = d.createElement('div');
		icon.className = ['ludonimbus-icon', cls('GameStatIcon'), cls('AchievementSVG')].filter(Boolean).join(' ');
		icon.innerHTML = TROPHY_SVG;
		const right = d.createElement('div');
		right.className = ['ludonimbus-text', cls('GameStatRight'), cls('AchievementRight')].filter(Boolean).join(' ');
		const label = d.createElement('div');
		label.className = ['ludonimbus-label', cls('PlayBarLabel'), cls('AchievementLabel')].filter(Boolean).join(' ');
		const row = d.createElement('div');
		row.className = cls('AchievementProgressRow') || 'ludonimbus-ach-row-inline';
		const count = d.createElement('div');
		count.className = ['ludonimbus-detail', cls('PlayBarDetailLabel'), cls('AchievementCountLabel')].filter(Boolean).join(' ');
		// Steam's progress bar (DetailsProgressContainer > DetailsProgressBar), or our own.
		const track = d.createElement('div');
		track.className = cls('DetailsProgressContainer') || 'ludonimbus-ach-plainbar';
		const fill = d.createElement('div');
		fill.className = ['ludonimbus-fill', cls('DetailsProgressBar')].filter(Boolean).join(' ');
		if (!cls('DetailsProgressBar')) {
			fill.style.height = '100%';
			fill.style.background = '#1a9fff';
		}
		track.appendChild(fill);
		row.append(count, track);
		right.append(label, row);
		item.append(icon, right);
		item.addEventListener('click', () => this.openList());
		return item;
	}

	/** The "Achievements" section at the top of the right column. */
	private renderSection(r: Shown): void {
		const sec = findClassModule((mod: any) => !!(mod.AppDetailsSection && mod.RightColumnSection && mod.AppDetailsSectionContainer)) as
			| Record<string, string>
			| undefined;
		if (!sec) return;
		const first = (c: string) => c.split(/\s+/)[0];
		const candidates: Element[] = [];
		for (const el of Array.from(this.doc.querySelectorAll(`.${first(sec.RightColumnSection)}`))) {
			if (el.closest(`[${MARK}]`)) continue;
			const column = el.closest(`.${first(sec.AppDetailsSection)}`)?.parentElement;
			if (column && !candidates.includes(column)) candidates.push(column);
		}
		// Steam wraps some sections (recordings and screenshots, ...) in a container
		// inside the right column; that container is not a column of its own. Use the
		// outermost one (a visible one if Steam keeps a hidden copy): one section only.
		const outer = candidates.filter((c) => !candidates.some((o) => o !== c && o.contains(c)));
		const column = outer.find((c) => c.getClientRects().length > 0) ?? outer[0];
		this.doc.querySelectorAll(`[${MARK}="section"]`).forEach((el) => {
			if (el.parentElement !== column) el.remove();
		});
		if (!column) return;
		let el = column.querySelector<HTMLElement>(`:scope > [${MARK}="section"]`);
		if (!el) el = this.createSection(sec);
		if (column.firstElementChild !== el) column.insertBefore(el, column.firstElementChild);
		this.fillSection(el, r);
	}

	private createSection(sec: Record<string, string>): HTMLElement {
		const d = this.doc;
		const head = findClassModule((mod: any) => !!(mod.SectionHeader && mod.LabelText && mod.PadLeft)) as Record<string, string> | undefined;
		const el = d.createElement('div');
		el.setAttribute(MARK, 'section');
		el.className = ['ludonimbus-ach-section', sec.AppDetailsSection].join(' ');
		const header = d.createElement('div');
		header.className = head ? [head.SectionHeader, head.PadLeft].join(' ') : 'ludonimbus-ach-sec-plainhead';
		const label = d.createElement('div');
		label.className = head?.Label ?? '';
		const text = d.createElement('div');
		text.className = ['ludonimbus-ach-sec-title', head?.LabelText ?? ''].join(' ');
		label.appendChild(text);
		header.appendChild(label);
		const body = d.createElement('div');
		body.className = ['ludonimbus-ach-sec-body', sec.AppDetailsSectionContainer, sec.AppDetailsSectionHasLabel, sec.RightColumnSection]
			.filter(Boolean)
			.join(' ');
		body.addEventListener('click', () => this.openList());
		el.append(header, body);
		return el;
	}

	private fillSection(el: HTMLElement, r: Shown): void {
		const m = tx();
		const items = r.items ?? [];
		const unlocked = items.filter((i) => i.unlockedAt);
		const locked = items.filter((i) => !i.unlockedAt);
		// Rebuild only when something shown changed (the observer calls this often).
		const sig = [lang, r.unlocked, r.total, r.gameIcon ? 1 : 0, unlocked.map((i) => i.id).join(',')].join('|');
		if (el.dataset.sig === sig) return;
		el.dataset.sig = sig;
		const title = el.querySelector('.ludonimbus-ach-sec-title');
		if (title) title.textContent = m.label;
		const body = el.querySelector<HTMLElement>('.ludonimbus-ach-sec-body');
		if (!body) return;
		const d = this.doc;
		const pct = Math.floor((100 * r.unlocked) / Math.max(1, r.total));
		body.replaceChildren();
		const summary = d.createElement('div');
		summary.className = 'ludonimbus-ach-sec-summary';
		summary.textContent = m.summary(r.unlocked, r.total, pct);
		const bar = d.createElement('div');
		bar.className = 'ludonimbus-ach-sec-progress';
		const fill = d.createElement('div');
		fill.style.width = `${pct}%`;
		bar.appendChild(fill);
		const grid = d.createElement('div');
		grid.className = 'ludonimbus-ach-sec-icons';
		// Newest unlocks first, then locked ones; the last tile counts the rest.
		const MAX = 12;
		const order = [...unlocked.slice(0, 6), ...locked, ...unlocked.slice(6)];
		const tiles = order.length > MAX ? order.slice(0, MAX - 1) : order;
		for (const a of tiles) {
			const isLocked = !a.unlockedAt;
			grid.appendChild(this.icon(a, r, isLocked && a.hidden ? m.hidden : a.name));
		}
		if (order.length > tiles.length) {
			const more = d.createElement('div');
			more.className = 'ludonimbus-ach-sec-rest';
			more.textContent = `+${order.length - tiles.length}`;
			grid.appendChild(more);
		}
		const all = d.createElement('div');
		all.className = 'ludonimbus-ach-sec-all';
		all.textContent = m.viewAll;
		body.append(summary, bar, grid, all);
	}

	private openList(): void {
		const r = this.reply;
		if (!shows(r)) return;
		const d = this.doc;
		const m = tx();
		d.querySelector('.ludonimbus-ach-backdrop')?.remove();
		const backdrop = d.createElement('div');
		backdrop.className = 'ludonimbus-ach-backdrop';
		backdrop.setAttribute(MARK, 'list');
		const panel = d.createElement('div');
		panel.className = 'ludonimbus-ach-panel';
		const close = () => {
			backdrop.remove();
			d.removeEventListener('keydown', onKey, true);
		};
		const onKey = (e: KeyboardEvent) => {
			if (e.key === 'Escape') {
				e.stopPropagation();
				close();
			}
		};
		d.addEventListener('keydown', onKey, true);
		backdrop.addEventListener('click', (e) => e.target === backdrop && close());

		const head = d.createElement('div');
		head.className = 'ludonimbus-ach-head';
		const title = d.createElement('div');
		title.className = 'ludonimbus-ach-title';
		title.innerHTML = TROPHY_SVG.replace('<svg', '<svg width="22" height="22"');
		const name = d.createElement('span');
		name.textContent = `${r.name ?? r.title ?? ''} · ${m.label}`;
		const btn = d.createElement('button');
		btn.className = 'ludonimbus-ach-close';
		btn.title = m.close;
		btn.textContent = '×';
		btn.addEventListener('click', close);
		title.append(name, btn);
		const pct = Math.floor((100 * r.unlocked) / Math.max(1, r.total));
		const sub = d.createElement('div');
		sub.className = 'ludonimbus-ach-sub';
		sub.textContent =
			m.unlockedOf(r.unlocked, r.total, pct) +
			(r.pointsTotal ? m.points(r.points ?? 0, r.pointsTotal) : '') +
			(r.sourceLabel ? m.from(r.sourceLabel) : '');
		const bar = d.createElement('div');
		bar.className = 'ludonimbus-ach-progress';
		const fill = d.createElement('div');
		fill.style.width = `${pct}%`;
		bar.appendChild(fill);
		head.append(title, sub, bar);

		const list = d.createElement('div');
		list.className = 'ludonimbus-ach-list';
		for (const a of r.items ?? []) list.appendChild(this.row(a, r));
		panel.append(head, list);
		backdrop.appendChild(panel);
		d.body.appendChild(backdrop);
	}

	/** The achievement's icon, else the game's, else a trophy. */
	private icon(a: AchievementItem, r: Shown, title: string): HTMLElement {
		const d = this.doc;
		if (!a.iconUrl && !a.hasIcon && !r.gameIcon) {
			const el = d.createElement('div');
			el.className = 'ludonimbus-ach-noicon';
			el.innerHTML = TROPHY_SVG;
			el.title = title;
			return el;
		}
		const img = d.createElement('img');
		img.alt = '';
		img.title = title;
		img.dataset.locked = a.unlockedAt ? '0' : '1';
		const useGame = () => gameIcon(r.key).then((u) => u && img.src !== u && (img.src = u));
		// A web icon that doesn't load (offline) falls back to the game's.
		img.addEventListener('error', () => useGame(), { once: true });
		if (a.iconUrl) img.src = a.iconUrl;
		else if (a.hasIcon) localIcon(r.key, a.id).then((u) => (u ? (img.src = u) : useGame()));
		else useGame();
		return img;
	}

	private row(a: AchievementItem, r: Shown): HTMLElement {
		const d = this.doc;
		const m = tx();
		const locked = !a.unlockedAt;
		const secret = locked && a.hidden;
		const row = d.createElement('div');
		row.className = 'ludonimbus-ach-row';
		row.dataset.locked = locked ? '1' : '0';
		const img = this.icon(a, r, '');
		const text = d.createElement('div');
		text.className = 'ludonimbus-ach-text';
		const name = d.createElement('div');
		name.className = 'ludonimbus-ach-name';
		name.textContent = secret ? m.hidden : a.name;
		if (a.grade) {
			const tag = d.createElement('span');
			tag.className = 'ludonimbus-ach-tag';
			tag.dataset.kind = a.grade;
			tag.textContent = m.grades[a.grade] ?? a.grade;
			name.appendChild(tag);
		}
		if (a.points != null) {
			const tag = d.createElement('span');
			tag.className = 'ludonimbus-ach-tag';
			tag.textContent = m.pts(a.points);
			name.appendChild(tag);
		}
		if (a.hardcore) {
			const tag = d.createElement('span');
			tag.className = 'ludonimbus-ach-tag';
			tag.textContent = m.hardcore;
			name.appendChild(tag);
		}
		const desc = d.createElement('div');
		desc.className = 'ludonimbus-ach-desc';
		desc.textContent = secret ? m.hiddenDesc : a.description;
		text.append(name, desc);
		const meta = d.createElement('div');
		meta.className = 'ludonimbus-ach-meta';
		meta.style.whiteSpace = 'pre';
		meta.textContent = a.unlockedAt ? m.on(when(a.unlockedAt), a.deviceName) : '';
		row.append(img, text, meta);
		return row;
	}
}
