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

export function relTime(iso: string | null | undefined): string {
	if (!iso) return '从未';
	const diff = (Date.now() - new Date(iso).getTime()) / 1000;
	if (diff < 45) return '刚刚';
	if (diff < 3600) return `${Math.round(diff / 60)} 分钟前`;
	if (diff < 86400) return `${Math.round(diff / 3600)} 小时前`;
	if (diff < 86400 * 30) return `${Math.round(diff / 86400)} 天前`;
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
	if ('error' in reply) {
		if (!isShortcutAppId(appId)) return null;
		if (reply.error === 'not_running') {
			return {
				text: 'Ludonimbus 未运行',
				tone: 'muted',
				tip: '打开 Ludonimbus 后显示这个游戏的云存档状态。',
				canSync: false,
				busy: false,
				key: null,
			};
		}
		return {
			text: '无法读取状态',
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
		tips.push(`上次同步：${dateTime(g.lastSyncAt)}${g.lastSyncedDevice ? `（${g.lastSyncedDevice}）` : ''}`);
	} else {
		tips.push('这台电脑还没有同步过');
	}
	if (g.cloudHead) tips.push(`云端最新：${dateTime(g.cloudHead.createdAt)}，来自 ${g.cloudHead.deviceName}`);
	if (g.pending > 0) tips.push(`${g.pending} 个版本等待上传`);
	if (g.lastError) tips.push(`上次出错：${g.lastError}`);
	if (!g.steamEnabled) tips.push('没有在 Steam 中开启自动同步：从 Steam 启动时不会自动同步');
	tips.push('点击在 Ludonimbus 中查看');
	const tip = tips.join('\n');
	const base = { tip, key: g.key, canSync: false, busy: false };

	switch (g.phase) {
		case 'preLaunch':
			return { ...base, text: '正在检查云存档…', tone: 'info', busy: true };
		case 'syncing':
			return { ...base, text: '正在同步…', tone: 'info', busy: true };
		case 'uploading':
			return { ...base, text: '正在上传存档…', tone: 'info', busy: true };
		case 'playing':
			return {
				...base,
				text: g.autoSync ? '游戏运行中，退出后自动上传' : '游戏运行中',
				tone: 'muted',
			};
	}

	if (BROKEN_SHORTCUT.includes(g.health)) {
		return { ...base, text: '快捷方式已更改，自动同步失效，点击修复', tone: 'warn' };
	}
	if (!g.autoSync) {
		return { ...base, text: '已关闭自动同步', tone: 'muted', canSync: true };
	}

	const when = g.lastSyncAt ? ` · ${relTime(g.lastSyncAt)}` : '';
	const canSync = true;
	switch (g.status) {
		case 'synced':
			return { ...base, text: `已是最新${when}`, tone: 'ok', canSync };
		case 'localChanged':
			return { ...base, text: '本机有新存档，等待上传', tone: 'info', canSync };
		case 'notUploaded':
			return { ...base, text: '存档尚未上传', tone: 'info', canSync };
		case 'cloudNewer':
			return {
				...base,
				text: g.cloudHead ? `云端有更新的存档（${g.cloudHead.deviceName}）` : '云端有更新的存档',
				tone: 'info',
				canSync,
			};
		case 'canRestore':
			return { ...base, text: '本机没有存档，可从云端恢复', tone: 'info', canSync };
		case 'conflict':
			return { ...base, text: '存档冲突，点击选择保留哪一份', tone: 'warn' };
		case 'offline':
			return {
				...base,
				text: g.pending > 0 ? `云端无法访问，${g.pending} 个版本待上传` : '云端暂时无法访问',
				tone: 'muted',
				canSync,
			};
		case 'noCloud':
			return { ...base, text: '还没有连接网盘', tone: 'muted' };
		case 'empty':
			return { ...base, text: '还没有存档', tone: 'muted' };
		case 'error':
			return { ...base, text: '读取存档出错', tone: 'bad', tip: `${g.message ?? ''}\n${tip}`.trim(), canSync };
		default:
			return { ...base, text: '正在检查…', tone: 'muted', busy: true };
	}
}
