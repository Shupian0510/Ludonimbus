// What the local API returns and how it is shown in the play bar.

export type Phase = 'idle' | 'syncing' | 'preLaunch' | 'playing' | 'uploading';

export type Status =
	| 'noCloud'
	| 'empty'
	| 'notUploaded'
	| 'synced'
	| 'localChanged'
	| 'cloudNewer'
	| 'canRestore'
	| 'conflict'
	| 'offline'
	| 'error';

export interface GameStatus {
	managed: true;
	key: string;
	name: string;
	autoSync: boolean;
	steamEnabled: boolean;
	health: string;
	phase: Phase;
	status: Status | null;
	message: string | null;
	lastSyncAt: string | null;
	lastSyncedDevice: string | null;
	cloudHead: { deviceName: string; createdAt: string } | null;
	pending: number;
	conflict: boolean;
	lastError: string | null;
	checkedAt: string | null;
	/** Ludonimbus's interface language. */
	lang?: string;
}

export type Reply = GameStatus | { managed: false } | { error: string; status?: number; detail?: string | null };

export type Tone = 'ok' | 'info' | 'warn' | 'bad' | 'muted';

export interface View {
	text: string;
	tone: Tone;
	tip: string;
	/** Show the "sync now" button. */
	canSync: boolean;
	/** Animate the icon (work in progress). */
	busy: boolean;
	/** Clicking opens the game in Ludonimbus. */
	key: string | null;
}

/**
 * The backend returns JSON text, but Millennium's FFI parses JSON results
 * before handing them over, so accept both.
 */
export function parseReply(value: unknown): Reply {
	let v = value;
	if (typeof value === 'string') {
		try {
			v = JSON.parse(value);
		} catch {
			return { error: 'bad_reply', detail: value.slice(0, 200) };
		}
	}
	if (v && typeof v === 'object') return v as Reply;
	return { error: 'bad_reply', detail: String(v) };
}

export function isGame(r: Reply | null): r is GameStatus {
	return !!r && 'managed' in r && r.managed === true;
}

/** Steam gives every non-Steam shortcut an app id with the top bit set. */
export function isShortcutAppId(appId: string): boolean {
	const n = Number(appId);
	return Number.isFinite(n) && n >= 0x80000000;
}

// ---------------------------------------------------------------------------
// Texts. The language follows Ludonimbus (every reply says which one); until
// the first reply, Steam's own language decides.

type Lang = 'zh-CN' | 'en';
let lang: Lang = (navigator.language || '').toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';

export function setLang(l: string | undefined): void {
	if (l === 'zh-CN' || l === 'en') lang = l;
}

const pl = (n: number, one: string, many: string) => (n === 1 ? one : many);

const TEXTS = {
	'zh-CN': {
		label: 'Ludonimbus 云存档',
		syncNow: '立即同步',
		connecting: '正在连接 Ludonimbus…',
		connected: (v: string) => `已连接 Ludonimbus ${v}`,
		notRunningLong: 'Ludonimbus 未运行，打开它后显示云存档状态',
		never: '从未',
		justNow: '刚刚',
		minutesAgo: (n: number) => `${n} 分钟前`,
		hoursAgo: (n: number) => `${n} 小时前`,
		daysAgo: (n: number) => `${n} 天前`,
		notRunning: 'Ludonimbus 未运行',
		notRunningTip: '打开 Ludonimbus 后显示这个游戏的云存档状态。',
		cantRead: '无法读取状态',
		lastSync: (at: string, device: string | null) => `上次同步：${at}${device ? `（${device}）` : ''}`,
		neverSynced: '这台电脑还没有同步过',
		cloudHead: (at: string, device: string) => `云端最新：${at}，来自 ${device}`,
		pending: (n: number) => `${n} 个版本等待上传`,
		lastError: (e: string) => `上次出错：${e}`,
		notEnabled: '没有在 Steam 中开启自动同步：从 Steam 启动时不会自动同步',
		clickToOpen: '点击在 Ludonimbus 中查看',
		preLaunch: '正在检查云存档…',
		syncing: '正在同步…',
		uploading: '正在上传存档…',
		playingAuto: '游戏运行中，退出后自动上传',
		playing: '游戏运行中',
		broken: '快捷方式已更改，自动同步失效，点击修复',
		autoOff: '已关闭自动同步',
		synced: (when: string) => `已是最新${when ? ` · ${when}` : ''}`,
		localChanged: '本机有新存档，等待上传',
		notUploaded: '存档尚未上传',
		cloudNewer: (device: string | null) => (device ? `云端有更新的存档（${device}）` : '云端有更新的存档'),
		canRestore: '本机没有存档，可从云端恢复',
		conflict: '存档冲突，点击选择保留哪一份',
		offline: (pending: number) => (pending > 0 ? `云端无法访问，${pending} 个版本待上传` : '云端暂时无法访问'),
		noCloud: '还没有连接网盘',
		empty: '还没有存档',
		error: '读取存档出错',
		checking: '正在检查…',
	},
	en: {
		label: 'Ludonimbus Cloud Saves',
		syncNow: 'Sync now',
		connecting: 'Connecting to Ludonimbus…',
		connected: (v: string) => `Connected to Ludonimbus ${v}`,
		notRunningLong: 'Ludonimbus is not running. Open it to see cloud save status',
		never: 'never',
		justNow: 'just now',
		minutesAgo: (n: number) => `${n} ${pl(n, 'minute', 'minutes')} ago`,
		hoursAgo: (n: number) => `${n} ${pl(n, 'hour', 'hours')} ago`,
		daysAgo: (n: number) => `${n} ${pl(n, 'day', 'days')} ago`,
		notRunning: 'Ludonimbus is not running',
		notRunningTip: "Open Ludonimbus to see this game's cloud save status.",
		cantRead: "Can't read the status",
		lastSync: (at: string, device: string | null) => `Last sync: ${at}${device ? ` (${device})` : ''}`,
		neverSynced: "This PC hasn't synced yet",
		cloudHead: (at: string, device: string) => `Latest in the cloud: ${at}, from ${device}`,
		pending: (n: number) => `${n} ${pl(n, 'version', 'versions')} waiting to upload`,
		lastError: (e: string) => `Last error: ${e}`,
		notEnabled: "Steam auto-sync is off: starting from Steam won't sync",
		clickToOpen: 'Click to open in Ludonimbus',
		preLaunch: 'Checking cloud saves…',
		syncing: 'Syncing…',
		uploading: 'Uploading saves…',
		playingAuto: 'Playing; saves upload when you quit',
		playing: 'Playing',
		broken: 'Shortcut changed; auto-sync is off. Click to repair',
		autoOff: 'Auto-sync is off',
		synced: (when: string) => `Up to date${when ? ` · ${when}` : ''}`,
		localChanged: 'New saves on this PC, waiting to upload',
		notUploaded: 'Saves not uploaded yet',
		cloudNewer: (device: string | null) => (device ? `Newer saves in the cloud (${device})` : 'Newer saves in the cloud'),
		canRestore: 'No saves on this PC; can restore from the cloud',
		conflict: 'Save conflict; click to choose which to keep',
		offline: (pending: number) =>
			pending > 0 ? `Cloud unreachable; ${pending} ${pl(pending, 'version', 'versions')} waiting` : "The cloud can't be reached right now",
		noCloud: 'No cloud connected yet',
		empty: 'No saves yet',
		error: 'Error reading saves',
		checking: 'Checking…',
	},
};

export function t() {
	return TEXTS[lang];
}

export function relTime(iso: string | null | undefined): string {
	const m = t();
	if (!iso) return m.never;
	const diff = (Date.now() - new Date(iso).getTime()) / 1000;
	if (diff < 45) return m.justNow;
	if (diff < 3600) return m.minutesAgo(Math.round(diff / 60));
	if (diff < 86400) return m.hoursAgo(Math.round(diff / 3600));
	if (diff < 86400 * 30) return m.daysAgo(Math.round(diff / 86400));
	return dateTime(iso);
}

function dateTime(iso: string): string {
	const d = new Date(iso);
	const p = (n: number) => String(n).padStart(2, '0');
	return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

const BROKEN_SHORTCUT = ['changed', 'runnerMoved', 'shortcutMissing'];

/** `null` = show nothing (a regular Steam game, or not known yet). */
export function describe(reply: Reply | null, appId: string): View | null {
	if (!reply) return null;
	if ('managed' in reply && reply.managed) setLang(reply.lang);
	const m = t();
	if ('error' in reply) {
		if (!isShortcutAppId(appId)) return null;
		if (reply.error === 'not_running') {
			return {
				text: m.notRunning,
				tone: 'muted',
				tip: m.notRunningTip,
				canSync: false,
				busy: false,
				key: null,
			};
		}
		return {
			text: m.cantRead,
			tone: 'muted',
			tip: reply.detail ?? reply.error,
			canSync: false,
			busy: false,
			key: null,
		};
	}
	if (!reply.managed) return null;

	const g = reply;
	const tips: string[] = [];
	if (g.lastSyncAt) {
		tips.push(m.lastSync(dateTime(g.lastSyncAt), g.lastSyncedDevice));
	} else {
		tips.push(m.neverSynced);
	}
	if (g.cloudHead) tips.push(m.cloudHead(dateTime(g.cloudHead.createdAt), g.cloudHead.deviceName));
	if (g.pending > 0) tips.push(m.pending(g.pending));
	if (g.lastError) tips.push(m.lastError(g.lastError));
	if (!g.steamEnabled) tips.push(m.notEnabled);
	tips.push(m.clickToOpen);
	const tip = tips.join('\n');
	const base = { tip, key: g.key, canSync: false, busy: false };

	switch (g.phase) {
		case 'preLaunch':
			return { ...base, text: m.preLaunch, tone: 'info', busy: true };
		case 'syncing':
			return { ...base, text: m.syncing, tone: 'info', busy: true };
		case 'uploading':
			return { ...base, text: m.uploading, tone: 'info', busy: true };
		case 'playing':
			return { ...base, text: g.autoSync ? m.playingAuto : m.playing, tone: 'muted' };
	}

	if (BROKEN_SHORTCUT.includes(g.health)) {
		return { ...base, text: m.broken, tone: 'warn' };
	}
	if (!g.autoSync) {
		return { ...base, text: m.autoOff, tone: 'muted', canSync: true };
	}

	const when = g.lastSyncAt ? relTime(g.lastSyncAt) : '';
	const canSync = true;
	switch (g.status) {
		case 'synced':
			return { ...base, text: m.synced(when), tone: 'ok', canSync };
		case 'localChanged':
			return { ...base, text: m.localChanged, tone: 'info', canSync };
		case 'notUploaded':
			return { ...base, text: m.notUploaded, tone: 'info', canSync };
		case 'cloudNewer':
			return { ...base, text: m.cloudNewer(g.cloudHead?.deviceName ?? null), tone: 'info', canSync };
		case 'canRestore':
			return { ...base, text: m.canRestore, tone: 'info', canSync };
		case 'conflict':
			return { ...base, text: m.conflict, tone: 'warn' };
		case 'offline':
			return { ...base, text: m.offline(g.pending), tone: 'muted', canSync };
		case 'noCloud':
			return { ...base, text: m.noCloud, tone: 'muted' };
		case 'empty':
			return { ...base, text: m.empty, tone: 'muted' };
		case 'error':
			return { ...base, text: m.error, tone: 'bad', tip: `${g.message ?? ''}\n${tip}`.trim(), canSync };
		default:
			return { ...base, text: m.checking, tone: 'muted', busy: true };
	}
}
