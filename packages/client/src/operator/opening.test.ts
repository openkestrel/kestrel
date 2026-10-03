import { describe, expect, test } from "vitest";
import type { Agent, Project } from "./generated";
import {
	resolvedBranch,
	resolvedEnvironment,
	resolvedModel,
	resolvedRepositories,
} from "./opening";

const a_project: Project = {
	id: "01a0a2d8-baf8-7c02-99fa-7280f174c14a",
	name: "kestrel",
	repositories: ["https://github.com/openkestrel/kestrel", "https://github.com/openkestrel/env"],
	branch: "main",
};

const an_agent: Agent = {
	id: "01a0a2d8-baf8-7c02-99fa-7280f174c14b",
	name: "builder",
	harness: "opencode",
	model: "claude-opus-5",
	mode: null,
	thought_level: null,
};

describe("the resolved values the form shows", () => {
	test("names the repositories and the branch the Project declares", () => {
		expect(resolvedRepositories(a_project)).toBe(
			"https://github.com/openkestrel/kestrel, https://github.com/openkestrel/env",
		);
		expect(resolvedBranch("", a_project)).toBe("main");
	});

	test("a branch the person names wins over the Project's", () => {
		expect(resolvedBranch("kestrel/topic", a_project)).toBe("kestrel/topic");
	});

	test("the model is the override, else the Agent's, else the harness's default", () => {
		expect(resolvedModel("scripted-max", an_agent)).toBe("scripted-max");
		expect(resolvedModel("", an_agent)).toBe("claude-opus-5");
		expect(resolvedModel("", { ...an_agent, model: null })).toBe("the harness's default");
	});

	test("the Environment is the driver the work role recorded, or says none is dispatching", () => {
		expect(resolvedEnvironment({ driver: "local-exec" })).toBe("local-exec");
		expect(resolvedEnvironment(null)).toBe("No dispatch configuration is recorded.");
	});
});
