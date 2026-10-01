// The name this browser takes turns under: one per browser, never sent anywhere until a turn is.
const KEY = "kestrel:declared-name";

export function declaredName(): string {
	try {
		return localStorage.getItem(KEY)?.trim() ?? "";
	} catch {
		return "";
	}
}

export function rememberDeclaredName(name: string): void {
	try {
		localStorage.setItem(KEY, name.trim());
	} catch {}
}
