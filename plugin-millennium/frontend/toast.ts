// Shows Ludonimbus achievement unlocks with Steam's own achievement popup.
//
// Steam's web UI (SharedJSContext, where this plugin runs) keeps its
// notification centre on `window.NotificationStore`. Its
// `GetNotificationTargets()` lists every notification type; the achievement
// one has `strTest: "Achievement"` and the protobuf class (`proto`) Steam
// decodes it with. That is exactly what Steam's own `TestAchievement` uses:
// build the message with `proto.fromObject`, serialise it, then
//   - `AddAppOverlayNotification(appid, …)`: into a running game's overlay;
//   - `OnNotification(…)`: the normal path (desktop corner, and the overlay of
//     the game being played).
// These are internal, undocumented APIs. Everything is looked up by name at
// run time; when something is missing the app is told, and it shows its own
// popup window instead. See docs/plan-achievements.md, section 4.

export interface AchievementEvent {
	seq: number;
	key: string;
	gameName: string;
	appid: number | null;
	id: string;
	name: string;
	description: string;
	icon: string | null;
	unlockedAt: string;
	hardcore: boolean;
	test: boolean;
}

export interface NotificationTarget {
	proto?: { fromObject(o: Record<string, unknown>): { serializeBinary(): Uint8Array } };
	strTest?: string;
}

export interface SteamNotificationStore {
	GetNotificationTargets?(): Record<string, NotificationTarget | undefined>;
	GetAppsWithOverlayRunning?(): number[];
	AddAppOverlayNotification?(appid: number, id: number, type: number, data: Uint8Array): void;
	OnNotification?(id: number, type: number, data: Uint8Array): void;
	m_nNextTestNotificationID?: number;
}

export type Shown = 'overlay' | 'desktop';

/** The achievement entry of Steam's notification registry. */
export function findAchievementTarget(store: SteamNotificationStore | undefined): { type: number; proto: NonNullable<NotificationTarget['proto']> } | null {
	const targets = store?.GetNotificationTargets?.();
	if (!targets) return null;
	for (const [type, t] of Object.entries(targets)) {
		if (t?.strTest === 'Achievement' && typeof t.proto?.fromObject === 'function' && Number.isFinite(Number(type))) {
			return { type: Number(type), proto: t.proto };
		}
	}
	return null;
}

let ownId = 0x4c4e0000;

function nextId(store: SteamNotificationStore): number {
	// Share Steam's counter for locally made notifications when it exists.
	if (typeof store.m_nNextTestNotificationID === 'number') return store.m_nNextTestNotificationID++;
	return ownId++;
}

/** The overlay to put the popup in: this game's, or the only one running. */
export function overlayFor(running: number[], appid: number): number | null {
	if (running.includes(appid)) return appid;
	return running.length === 1 ? running[0] : null;
}

/** Show one unlock. Throws a short reason when Steam can't show it. */
export function showSteamAchievement(store: SteamNotificationStore | undefined, ev: AchievementEvent): Shown {
	if (!store) throw new Error('NotificationStore not found');
	const target = findAchievementTarget(store);
	if (!target) throw new Error('achievement notification type not found');
	if (!ev.appid) throw new Error('game is not in Steam on this PC');
	const unlocked = Math.floor(Date.parse(ev.unlockedAt) / 1000);
	const data = target.proto
		.fromObject({
			achievement_id: ev.id,
			appid: ev.appid,
			name: ev.name,
			description: ev.description,
			image_url: ev.icon ?? '',
			achieved: true,
			rtime_unlocked: Number.isFinite(unlocked) ? unlocked : Math.floor(Date.now() / 1000),
			current_progress: 0,
			max_progress: 0,
			min_progress: 0,
			// Neutral: a low value makes Steam draw the "rare achievement" glow.
			global_achieved_pct: 50,
		})
		.serializeBinary();
	const id = nextId(store);
	const overlay = overlayFor(store.GetAppsWithOverlayRunning?.() ?? [], ev.appid);
	if (overlay !== null && typeof store.AddAppOverlayNotification === 'function') {
		store.AddAppOverlayNotification(overlay, id, target.type, data);
		return 'overlay';
	}
	if (typeof store.OnNotification === 'function') {
		store.OnNotification(id, target.type, data);
		return 'desktop';
	}
	throw new Error('no way to deliver the notification');
}

// ---------------------------------------------------------------------------
// Polling the app for new unlocks

const POLL_MS = 2000;

interface EventsReply {
	seq?: number;
	events?: AchievementEvent[];
	error?: string;
}

function parse(value: unknown): EventsReply {
	if (typeof value === 'string') {
		try {
			return JSON.parse(value) as EventsReply;
		} catch {
			return { error: 'bad_reply' };
		}
	}
	return (value ?? {}) as EventsReply;
}

export class AchievementToasts {
	private seq = 0;
	private timer: number | undefined;
	private stopped = false;

	start(): void {
		this.stopped = false;
		this.tick();
	}

	stop(): void {
		this.stopped = true;
		window.clearTimeout(this.timer);
	}

	private async tick(): Promise<void> {
		try {
			const reply = parse(await backend.get_events(String(this.seq)));
			if (!reply.error && !this.stopped) {
				for (const ev of reply.events ?? []) await this.show(ev);
				// Always the app's number, also when it is lower (an app from before
				// numbers continued across restarts): what follows is then seen.
				if (typeof reply.seq === 'number') this.seq = reply.seq;
			}
		} catch {
			/* app not running: try again later */
		}
		if (!this.stopped) this.timer = window.setTimeout(() => this.tick(), POLL_MS);
	}

	private async show(ev: AchievementEvent): Promise<void> {
		let shown = 'failed';
		let reason = '';
		try {
			reason = showSteamAchievement((window as any).NotificationStore, ev);
			shown = 'steam';
			console.log(`[Ludonimbus] achievement ${ev.key}/${ev.id} shown (${reason})`);
		} catch (e) {
			reason = e instanceof Error ? e.message : String(e);
			console.warn(`[Ludonimbus] achievement ${ev.key}/${ev.id} not shown in Steam: ${reason}`);
		}
		try {
			await backend.ack_event(String(ev.seq), shown, reason);
		} catch {
			/* ignore */
		}
		// Game pages refresh their achievement count.
		window.dispatchEvent(new CustomEvent('ludonimbus-achievement', { detail: ev.key }));
	}
}
