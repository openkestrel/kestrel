import { describe, expect, it } from "vitest";
import type { GithubAppFlow, Integration, Project } from "./generated";
import {
	A_NEW_PROJECT,
	authorities,
	chosenRepository,
	draftOfProject,
	expiresIn,
	flowStage,
	projectDeclaration,
	resolutionRequest,
} from "./repositories";

const flow: GithubAppFlow = {
	flow: "s3cret-free-reference",
	phase: "select_repository",
	organization: "acme",
	name: "github",
	expires_at: "2026-10-30T12:00:00Z",
	installation_id: 23,
	integration: null,
	repository: null,
	guidance: "Create and install the App, then explicitly select a repository.",
};

function integration(name: string, patch: Partial<Integration> = {}): Integration {
	return {
		id: "00000000-0000-0000-0000-00000000000a",
		name,
		kind: "github",
		repository: "acme/repo",
		bot_login: "kestrel[bot]",
		carries: ["inbound", "outbound"],
		state: "enabled",
		revision: 1,
		disabled_at: null,
		retired_at: null,
		canceled_posts: [],
		polled_every: "1m",
		webhook_path: null,
		last_event_refusal: null,
		diagnostic: null,
		webhook_secret_rotation: null,
		...patch,
	};
}

describe("projectDeclaration", () => {
	it("selects an Integration only for the repositories given one", () => {
		expect(
			projectDeclaration({
				name: " kestrel ",
				repositories: [
					{ address: " acme/repo ", integration: "github" },
					{ address: "https://example.com/docs.git", integration: "" },
					{ address: "  ", integration: "" },
				],
				branch: " main ",
			}),
		).toEqual({
			name: "kestrel",
			repositories: ["acme/repo", "https://example.com/docs.git"],
			integrations: [{ repository: "acme/repo", integration: "github" }],
			branch: "main",
		});
	});

	it("selects none when no repository names an Integration", () => {
		expect(
			projectDeclaration({
				name: "docs",
				repositories: [{ address: "acme/docs", integration: "" }],
				branch: "trunk",
			}).integrations,
		).toEqual([]);
	});
});

describe("resolutionRequest", () => {
	it("asks public setup to resolve without any Integration", () => {
		expect(
			resolutionRequest(
				{ ...A_NEW_PROJECT, repositories: [{ address: "acme/repo", integration: "" }] },
				"acme",
				"public_setup",
			),
		).toEqual({ repositories: ["acme/repo"], purpose: "public_setup" });
	});

	it("names the Organization whose Integrations a declaration selects", () => {
		expect(
			resolutionRequest(
				{
					name: "kestrel",
					branch: "main",
					repositories: [{ address: "git@github.com:acme/repo.git", integration: "github" }],
				},
				"acme",
				"declaration",
			),
		).toEqual({
			repositories: ["git@github.com:acme/repo.git"],
			purpose: "declaration",
			organization: "acme",
			integrations: [{ repository: "git@github.com:acme/repo.git", integration: "github" }],
		});
	});
});

describe("draftOfProject", () => {
	it("keeps each repository's selected Integration beside it", () => {
		const project: Project = {
			id: "00000000-0000-0000-0000-000000000001",
			name: "kestrel",
			repositories: ["https://github.com/acme/repo.git", "https://github.com/acme/docs.git"],
			integrations: [
				{
					repository: "https://github.com/acme/repo.git",
					integration: "github",
					integration_id: "00000000-0000-0000-0000-00000000000a",
				},
			],
			branch: "main",
		};
		expect(draftOfProject(project)).toEqual({
			name: "kestrel",
			branch: "main",
			repositories: [
				{ address: "https://github.com/acme/repo.git", integration: "github" },
				{ address: "https://github.com/acme/docs.git", integration: "" },
			],
		});
	});
});

describe("chosenRepository", () => {
	it("takes the repository and its default branch as GitHub named them, under the flow's Integration", () => {
		expect(
			chosenRepository(
				{ ...flow, phase: "completed", integration: "github", repository: "acme/repo" },
				{ id: 1, repository: "acme/repo", default_branch: "trunk", private: true },
			),
		).toEqual({
			name: "repo",
			branch: "trunk",
			repositories: [{ address: "acme/repo", integration: "github" }],
		});
	});
});

describe("flowStage", () => {
	it("waits for the installation before any repository can be chosen", () => {
		expect(flowStage({ ...flow, phase: "ready" })).toBe("installing");
		expect(flowStage({ ...flow, phase: "exchanging" })).toBe("installing");
		expect(flowStage({ ...flow, phase: "converted" })).toBe("installing");
		expect(flowStage(flow)).toBe("choosing");
	});

	it("is connected only once the flow names its Integration", () => {
		expect(flowStage({ ...flow, phase: "completed", integration: "github" })).toBe("connected");
	});

	it("ends on cancellation, expiry and a failed conversion alike", () => {
		for (const phase of ["cancelled", "expired", "conversion_failed"] as const)
			expect(flowStage({ ...flow, phase })).toBe("ended");
	});
});

describe("expiresIn", () => {
	it("says how long is left in whole minutes, and when it has passed", () => {
		const expires = "2026-10-30T12:00:00Z";
		expect(expiresIn(expires, Date.parse("2026-10-30T11:15:30Z"))).toBe("Expires in 45 minutes");
		expect(expiresIn(expires, Date.parse("2026-10-30T11:59:10Z"))).toBe("Expires in 1 minute");
		expect(expiresIn(expires, Date.parse("2026-10-30T12:00:01Z"))).toBe("Expired");
	});
});

describe("authorities", () => {
	it("offers only enabled GitHub Integrations that carry outbound", () => {
		expect(
			authorities([
				integration("github"),
				integration("paused", { state: "disabled" }),
				integration("inbound", { carries: ["inbound"] }),
				integration("hook", { kind: "webhook", repository: null }),
			]).map(({ name }) => name),
		).toEqual(["github"]);
	});
});
