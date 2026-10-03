// Deliberately in memory only: a reload starts the form empty.
export type NewWorkspaceDraft = {
	project: string;
	agent: string;
	profile: string;
	branch: string;
	model: string;
	brief: string;
	options: boolean;
	continues: string;
};

export const AN_EMPTY_DRAFT: NewWorkspaceDraft = {
	project: "",
	agent: "",
	profile: "",
	branch: "",
	model: "",
	brief: "",
	options: false,
	continues: "",
};

const drafts = new Map<string, NewWorkspaceDraft>();

export function draftOf(organization: string): NewWorkspaceDraft | undefined {
	return drafts.get(organization);
}

export function holdDraft(organization: string, draft: NewWorkspaceDraft): void {
	drafts.set(organization, draft);
}
