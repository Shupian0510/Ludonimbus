// Ludonimbus Steam plugin: shows the cloud save status of non-Steam games
// managed by Ludonimbus in the play bar of their library page.

import { definePlugin, Field, Millennium } from 'millennium';
import { useEffect, useState } from 'react';
import { AchievementsBar } from './achievements';
import { PlayBarStatus } from './playbar';
import { parseReply, setLang, t } from './status';
import { SteamPlaytime } from './steamplaytime';
import { AchievementToasts } from './toast';

const bars = new Map<Document, PlayBarStatus>();
const achievementBars = new Map<Document, AchievementsBar>();
const toasts = new AchievementToasts();
const steamPlaytime = new SteamPlaytime();

/** Stop what runs for a window: its timers, observers and injected items. */
function detach(doc: Document): void {
	bars.get(doc)?.stop();
	bars.delete(doc);
	achievementBars.get(doc)?.stop();
	achievementBars.delete(doc);
}

function attach(name: string | undefined, doc: Document | undefined): void {
	// Only the desktop library window ("SP Desktop_uid0"), not Big Picture.
	if (!name?.startsWith('SP Desktop') || !doc?.body || bars.has(doc)) return;
	for (const old of Array.from(bars.keys())) {
		if (!old.defaultView || old.defaultView.closed) detach(old);
	}
	const bar = new PlayBarStatus(doc);
	bars.set(doc, bar);
	bar.start();
	const achievements = new AchievementsBar(doc);
	achievementBars.set(doc, achievements);
	achievements.start();
	// The window closes (Steam to the tray): stop now, not at the next window.
	doc.defaultView?.addEventListener('unload', () => detach(doc), { once: true });
}

/** Windows that already existed when the plugin loaded. */
function attachExisting(): void {
	try {
		const popups: Map<string, any> | undefined = (window as any).g_PopupManager?.m_mapPopups;
		popups?.forEach((p: any) => attach(p?.m_strName, p?.m_popup?.document));
	} catch {
		/* not available in this Steam build */
	}
}

const QuickAccess = () => {
	const [state, setState] = useState(t().connecting);
	useEffect(() => {
		backend
			.ping()
			.then((raw) => {
				const v = parseReply(raw) as { version?: string; lang?: string };
				setLang(v.lang);
				setState(v.version ? t().connected(v.version) : t().notRunningLong);
			})
			.catch(() => setState(t().notRunningLong));
	}, []);
	return <Field label={t().label} description={state} />;
};

export default definePlugin(() => {
	Millennium.AddWindowCreateHook?.((ctx: any) => attach(ctx?.m_strName, ctx?.m_popup?.document));
	attachExisting();
	// New achievement unlocks: shown with Steam's own popup (toast.ts).
	toasts.start();
	steamPlaytime.start();

	return {
		title: t().label,
		icon: <div style={{ width: 16, height: 16 }}>☁</div>,
		content: <QuickAccess />,
		// The plugin is unloaded: nothing keeps running, Steam's playtimes come back.
		onDismount: () => {
			for (const doc of Array.from(bars.keys())) detach(doc);
			toasts.stop();
			steamPlaytime.stop();
		},
	};
});
