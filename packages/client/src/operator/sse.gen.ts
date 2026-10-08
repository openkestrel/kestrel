// Generated from openapi/operator.json by the generate binary.
export const ChangesEventNames = {
	Change: "change",
	ChangesOpen: "open",
	ChangesResync: "resync",
} as const;
export type ChangesEventData = {
	change: import("./generated").Change;
	open: import("./generated").ChangesOpen;
	resync: import("./generated").ChangesResync;
};
export const EventNames = {
	Activity: "activity",
	TranscriptSessionState: "session_state",
	Recorded: "entry",
	End: "end",
	CursorEvent: "cursor",
	FollowerEvent: "follower",
	Presence: "presence",
} as const;
export type EventData = {
	activity: import("./generated").Activity;
	session_state: import("./generated").TranscriptSessionState;
	entry: import("./generated").Recorded;
	end: import("./generated").End;
	cursor: import("./generated").CursorEvent;
	follower: import("./generated").FollowerEvent;
	presence: import("./generated").Presence;
};
