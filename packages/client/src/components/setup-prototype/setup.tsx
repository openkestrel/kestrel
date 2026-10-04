import {
	ArrowLeft,
	ArrowRight,
	Check,
	ChevronRight,
	Circle,
	ExternalLink,
	Settings,
} from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Conversation, ConversationContent } from "#/components/ai-elements/conversation";
import {
	ModelSelector,
	ModelSelectorTrigger,
	ModelSelectorContent,
	ModelSelectorInput,
	ModelSelectorList,
	ModelSelectorGroup,
	ModelSelectorItem,
	ModelSelectorName,
	ModelSelectorEmpty,
} from "#/components/ai-elements/model-selector";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import {
	Questionnaire,
	QuestionnaireItem,
	QuestionnaireTitle,
	QuestionnaireDescription,
	QuestionnaireChoices,
	QuestionnaireChoice,
} from "#/components/ui/questionnaire";
import { SetupSwitcher } from "./switcher";

const STEPS = ["Your name", "Harness", "Sign in", "Repository", "Brief", "Review"];
const SCENES = {
	empty: "Empty install",
	offline: "Control plane unreachable",
	waiting: "Relay waiting",
	checking: "Checking sign-in",
	success: "Sign-in verified",
	failed: "Relay failed",
	expired: "Relay expired",
	cancelled: "Relay cancelled",
	coverage: "Plan doesn't cover model",
	auth: "Authentication failed",
	unchecked: "Check inconclusive",
	ready: "Ready / no Workspaces",
	warning: "Unused broken sign-in",
	blocked: "Selected sign-in needs attention",
	image: "Harness absent from image",
	project: "No repository",
	github: "GitHub access lost",
	readonly: "Public repository / can't push",
	resumed: "Setup interrupted after name",
	signins: "Settings / Sign-ins",
	agents: "Settings / Agents",
	repositories: "Settings / Repositories",
};
const HARNESS = {
	claude: {
		label: "Claude Code",
		provider: "Anthropic",
		models: ["Default model", "Claude Sonnet", "Claude Opus"],
	},
	codex: {
		label: "Codex",
		provider: "OpenAI",
		models: ["Default model", "GPT-5.4", "GPT-5.4 mini"],
	},
	opencode: {
		label: "OpenCode",
		provider: "Provider",
		models: ["Default model", "Claude Sonnet", "GPT-5.4"],
	},
};
type Harness = keyof typeof HARNESS;
const HARNESSES: Harness[] = ["claude", "codex", "opencode"];
type Relay =
	| "idle"
	| "waiting"
	| "checking"
	| "success"
	| "failed"
	| "expired"
	| "cancelled"
	| "coverage"
	| "auth"
	| "unchecked";
type Page = "setup" | "workspaces" | "signins" | "agents" | "repositories";
type Variant = "A" | "B" | "C";
type Search = { variant: Variant; scene: string; theme: "light" | "dark" };
const VARIANTS = { A: "One question at a time", B: "Setup checklist", C: "Guided conversation" };

export function SetupPrototype({
	variant,
	scene,
	theme,
	onSearch,
}: Search & { onSearch: (next: Partial<Search>) => void }) {
	const fixture = scene in SCENES ? scene : "empty";
	const relayStates: Relay[] = [
		"waiting",
		"checking",
		"success",
		"failed",
		"expired",
		"cancelled",
		"coverage",
		"auth",
		"unchecked",
	];
	const initialRelay = relayStates.find((value) => value === fixture) ?? "idle";
	const [step, setStep] = useState(initialRelay !== "idle" ? 2 : fixture === "resumed" ? 1 : 0);
	const [page, setPage] = useState<Page>(
		((["signins", "agents", "repositories"] as const).find((value) => value === fixture) ??
			[
				"empty",
				"resumed",
				"waiting",
				"checking",
				"success",
				"failed",
				"expired",
				"cancelled",
				"coverage",
				"auth",
				"unchecked",
			].includes(fixture))
			? "setup"
			: "workspaces",
	);
	const [name, setName] = useState(fixture === "empty" ? "" : "Jack");
	const [harness, setHarness] = useState<Harness>("claude");
	const [method, setMethod] = useState("subscription");
	const [relay, setRelay] = useState<Relay>(initialRelay);
	const [signedIn, setSignedIn] = useState(page !== "setup" || initialRelay === "success");
	const [connected, setConnected] = useState(false);
	const [repository, setRepository] = useState(
		page !== "setup" && fixture !== "project" ? "openkestrel/kestrel" : "",
	);
	const [publicUrl, setPublicUrl] = useState("");
	const [readOnly, setReadOnly] = useState(fixture === "readonly");
	const [brief, setBrief] = useState("");
	const [model, setModel] = useState("Default model");
	const [modelOpen, setModelOpen] = useState(false);
	const [gap, setGap] = useState(fixture);
	const [started, setStarted] = useState(false);
	const [secret, setSecret] = useState("");
	const [manual, setManual] = useState(false);
	const [repoQuery, setRepoQuery] = useState("");
	const heading = useRef<HTMLHeadingElement>(null);
	const isBlocked = ["blocked", "image", "project"].includes(gap);
	const usedSignIn =
		method === "subscription"
			? `${name || "Your"}’s ${HARNESS[harness].label} subscription`
			: `${HARNESS[harness].provider} API key · Organization`;

	useEffect(() => {
		document.documentElement.dataset.theme = theme;
		document.documentElement.style.setProperty("--prototype-sans", "'Geist Variable'");
		document.documentElement.style.setProperty("--prototype-mono", "'Geist Mono Variable'");
	}, [theme]);
	useEffect(() => {
		heading.current?.focus({ preventScroll: page === "setup" && step > 0 });
	}, [step, page]);
	function chooseHarness(next: Harness) {
		setHarness(next);
		setMethod("subscription");
		setSignedIn(false);
		setRelay("idle");
		setSecret("");
		setModel("Default model");
		setManual(false);
	}
	function finishCheck() {
		setSecret("");
		setRelay("checking");
	}
	function repairSignIn() {
		setGap("ready");
		setSignedIn(true);
		setPage("workspaces");
	}

	const Model = (
		<ModelSelector open={modelOpen} onOpenChange={setModelOpen}>
			<ModelSelectorTrigger
				render={
					<Button variant="ghost">
						<ModelSelectorName>{model}</ModelSelectorName>
						<ChevronRight className="size-3" />
					</Button>
				}
			/>
			<ModelSelectorContent title="Choose a model">
				<ModelSelectorInput placeholder="Search models…" />
				<ModelSelectorList>
					<ModelSelectorEmpty>No model matches.</ModelSelectorEmpty>
					<ModelSelectorGroup heading={HARNESS[harness].provider}>
						{HARNESS[harness].models.map((one) => (
							<ModelSelectorItem
								key={one}
								value={one}
								onSelect={() => {
									setModel(one);
									setModelOpen(false);
								}}
							>
								{one}
								{model === one && <Check className="ml-auto size-4" />}
							</ModelSelectorItem>
						))}
					</ModelSelectorGroup>
				</ModelSelectorList>
			</ModelSelectorContent>
		</ModelSelector>
	);

	const SignIn = (
		<div className="grid gap-5">
			<fieldset className="flex gap-2" aria-label="Sign-in Method">
				{["subscription", "key"].map((one) => (
					<Button
						key={one}
						variant={method === one ? "default" : "outline"}
						onClick={() => {
							setMethod(one);
							setRelay("idle");
							setSignedIn(false);
							setSecret("");
						}}
					>
						{one === "key" ? "API key" : harness === "opencode" ? "Go / Zen" : "Subscription"}
					</Button>
				))}
			</fieldset>
			<p className="text-sm text-muted-foreground">
				{method === "subscription"
					? `Held in ${name || "your"}’s Subscription Profile. Only used for work you open.`
					: "Held by the Organization. Available when you have no subscription for this harness."}
			</p>
			{harness === "claude" && method === "subscription" && (
				<p className="text-sm">Your Claude plan serves only your own work.</p>
			)}
			{relay === "idle" && method === "subscription" && harness !== "opencode" && !manual && (
				<Button className="w-fit" onClick={() => setRelay("waiting")}>
					Sign in with {HARNESS[harness].label}
					<ExternalLink className="ml-2 size-3" />
				</Button>
			)}
			{relay === "waiting" && (
				<div className="grid gap-3 border-l-2 pl-4" aria-live="polite">
					<strong>
						{harness === "codex"
							? "Enter this code in your browser"
							: "Finish signing in in your browser"}
					</strong>
					<p className="text-sm text-muted-foreground">
						{harness === "codex"
							? "Then return here. kestrel will notice when you finish."
							: "Then paste the returned code here."}
					</p>
					<code className="break-all text-xs">https://sign-in.example.invalid/{harness}</code>
					{harness === "codex" ? (
						<code className="text-xl">ABCD-EFGH</code>
					) : (
						<Field label="Code from the sign-in page" value={secret} onChange={setSecret} secret />
					)}
					<p className="text-xs text-muted-foreground">This attempt ends after 10 minutes.</p>
					<div className="flex gap-2">
						{harness !== "codex" && (
							<Button disabled={!secret.trim()} onClick={finishCheck}>
								Submit code
							</Button>
						)}
						<Button
							variant="outline"
							onClick={() => {
								setRelay("cancelled");
								setSecret("");
							}}
						>
							Cancel sign-in
						</Button>
					</div>
				</div>
			)}
			{relay === "checking" && (
				<Notice title="Checking your sign-in…">
					kestrel is making one real call before saving this as usable.
				</Notice>
			)}
			{relay === "success" && (
				<Notice title="Signed in">
					<p>{usedSignIn}</p>
					<p className="text-xs text-muted-foreground">Verified just now · ready to use</p>
				</Notice>
			)}
			{["failed", "expired", "cancelled", "coverage", "auth", "unchecked"].includes(relay) && (
				<Notice
					title={
						(
							{
								failed: "The sign-in relay stopped",
								expired: "This sign-in attempt expired",
								cancelled: "Sign-in cancelled",
								coverage: "Your plan doesn't cover this model",
								auth: "Authentication failed",
								unchecked: "We couldn't check this sign-in",
							} as Record<string, string>
						)[relay]
					}
				>
					<p>
						{relay === "auth"
							? "The check reported authentication_required. It did not establish expiry or plan coverage."
							: relay === "unchecked"
								? "The check timed out. This does not establish an authentication failure."
								: relay === "coverage"
									? "The check returned plan-coverage evidence. Try the default model or another sign-in."
									: "No usable sign-in was saved. Try again or supply your sign-in manually."}
					</p>
					<div className="mt-3 flex flex-wrap gap-2">
						<Button
							onClick={() => {
								setRelay("idle");
								setManual(false);
							}}
						>
							Try again
						</Button>
						<Button
							variant="outline"
							onClick={() => {
								setManual(true);
								setRelay("idle");
							}}
						>
							Supply manually
						</Button>
					</div>
				</Notice>
			)}
			{(method === "key" || harness === "opencode" || manual) && relay === "idle" && (
				<div className="grid gap-3">
					{harness === "opencode" && method === "subscription" && (
						<p className="text-sm">
							Open the Go / Zen console and copy its key.{" "}
							<span className="text-muted-foreground">
								(External console link in the real flow.)
							</span>
						</p>
					)}
					<Field
						label={
							method === "key" || harness === "opencode"
								? "API key"
								: harness === "codex"
									? "Contents of your Codex login file"
									: "Claude subscription token"
						}
						secret
						value={secret}
						onChange={setSecret}
					/>
					<Button className="w-fit" disabled={!secret.trim()} onClick={finishCheck}>
						Save and check sign-in
					</Button>
					<p className="text-xs text-muted-foreground">
						Use any text here. All credentials in this prototype are discarded.
					</p>
				</div>
			)}
			{relay === "idle" && !manual && method === "subscription" && harness !== "opencode" && (
				<Button className="w-fit" variant="link" onClick={() => setManual(true)}>
					Already have a token or login file?
				</Button>
			)}
			{import.meta.env.DEV && (relay === "waiting" || relay === "checking") && (
				<details className="border border-dashed p-3 text-xs">
					<summary>Prototype controls · simulate external result</summary>
					<div className="mt-3 flex flex-wrap gap-2">
						{(["success", "failed", "expired", "coverage", "auth", "unchecked"] as Relay[]).map(
							(result) => (
								<Button
									size="xs"
									variant="outline"
									key={result}
									onClick={() => {
										setRelay(result);
										setSignedIn(result === "success");
										setSecret("");
									}}
								>
									{result}
								</Button>
							),
						)}
					</div>
				</details>
			)}
		</div>
	);

	const Repository = (
		<div className="grid gap-5">
			<p className="text-sm text-muted-foreground">
				Connect GitHub so your Session can clone, push and open a pull request.
			</p>
			{!connected ? (
				<>
					<Button className="w-fit" onClick={() => setConnected(true)}>
						Connect GitHub
						<ExternalLink className="ml-2 size-3" />
					</Button>
					<p className="text-xs text-muted-foreground">
						The real flow opens GitHub to create and install kestrel’s GitHub App. This button
						simulates the completed installation.
					</p>
				</>
			) : (
				<>
					<Notice title="GitHub connected">The App can access these repositories.</Notice>
					<Field label="Find a repository" value={repoQuery} onChange={setRepoQuery} />
					<fieldset aria-label="Repositories" className="grid gap-2">
						{["openkestrel/kestrel", "jack/notes"]
							.filter((repo) => repo.includes(repoQuery))
							.map((repo) => (
								<Button
									size="lg"
									className="justify-between"
									variant={repository === repo ? "secondary" : "outline"}
									key={repo}
									onClick={() => {
										setRepository(repo);
										setReadOnly(false);
									}}
								>
									<span>{repo}</span>
									<span className="font-mono text-xs text-muted-foreground">
										main {repository === repo && "✓"}
									</span>
								</Button>
							))}
					</fieldset>
				</>
			)}
			<details>
				<summary className="cursor-pointer text-sm">
					Use a public repository without connecting GitHub
				</summary>
				<div className="mt-3 grid gap-3">
					<Field
						label="Public repository URL"
						value={publicUrl}
						onChange={setPublicUrl}
						placeholder="https://github.com/owner/repository"
					/>
					<Button
						className="w-fit"
						variant="outline"
						disabled={!/^https:\/\/github\.com\/[^/]+\/[^/]+\/?$/.test(publicUrl)}
						onClick={() => {
							setRepository(publicUrl.replace("https://github.com/", "").replace(/\/$/, ""));
							setReadOnly(true);
						}}
					>
						Use this repository
					</Button>
				</div>
			</details>
			{repository && (
				<p className="text-sm">
					Selected: <strong>{repository}</strong> · main
				</p>
			)}
			{readOnly && (
				<Notice title="This repository is read-only">
					Your Session can read and change its checkout, but cannot push or open a pull request.
					Connect GitHub to enable that.
				</Notice>
			)}
		</div>
	);

	const Review = (
		<div className="grid gap-4">
			<p className="text-sm text-muted-foreground">
				Here’s what kestrel will create. Nothing starts until you confirm.
			</p>
			<dl className="grid gap-3 text-sm">
				{[
					["Operator", name],
					["Agent", HARNESS[harness].label],
					["Model", model],
					["Sign-in", usedSignIn],
					["Project", `${repository} · main`],
					["Organization", "Default Organization"],
				].map(([label, value]) => (
					<div className="flex flex-wrap justify-between gap-2 border-b pb-2" key={label}>
						<dt className="text-muted-foreground">{label}</dt>
						<dd>{value}</dd>
					</div>
				))}
			</dl>
			<blockquote className="whitespace-pre-wrap border-l-2 pl-3 text-sm">{brief}</blockquote>
			{readOnly && (
				<Notice title="This Session cannot push">
					Connect GitHub when you want it to open a pull request.
				</Notice>
			)}
			<p className="text-xs text-muted-foreground">
				Create the default Organization, Agent and Project; open a Workspace and queue its first
				Session.
			</p>
		</div>
	);
	const stepBody = [
		<Field
			key="name"
			label="What should kestrel call you?"
			value={name}
			onChange={setName}
			placeholder="Your name"
		/>,
		<div key="harness" className="grid gap-4">
			<QuestionnaireChoices>
				{HARNESSES.map((one) => (
					<QuestionnaireChoice
						key={one}
						value={one}
						checked={harness === one}
						onChange={() => chooseHarness(one)}
					>
						{HARNESS[one].label}
					</QuestionnaireChoice>
				))}
			</QuestionnaireChoices>
			<p className="text-sm text-muted-foreground">
				All three are available in the default image. Use the harness’s default model, or choose
				another.
			</p>
			{Model}
		</div>,
		SignIn,
		Repository,
		<div key="brief" className="grid gap-3">
			<label htmlFor="brief" className="text-sm">
				What would you like your first Session to work on?
			</label>
			<textarea
				id="brief"
				className="min-h-36 w-full border bg-background p-3 text-sm outline-ring"
				value={brief}
				onChange={(event) => setBrief(event.target.value)}
				placeholder="Describe the work…"
			/>
			<div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
				{HARNESS[harness].label} · {Model}
			</div>
		</div>,
		Review,
	][step];
	const canContinue = [!!name.trim(), true, signedIn, !!repository, !!brief.trim(), true][step];
	const Question = (
		<Questionnaire key={step} item={String(step)} onSubmit={(event) => event.preventDefault()}>
			<QuestionnaireItem name={String(step)}>
				<QuestionnaireTitle>
					<h1
						ref={heading}
						tabIndex={-1}
						className="text-2xl font-medium tracking-tight outline-none"
					>
						{STEPS[step]}
					</h1>
				</QuestionnaireTitle>
				{step === 0 && (
					<QuestionnaireDescription>
						This name follows your messages and your own subscription. You can change it later.
					</QuestionnaireDescription>
				)}
				{stepBody}
			</QuestionnaireItem>
			<div className="mt-3 flex justify-between gap-3">
				<Button variant="ghost" disabled={step === 0} onClick={() => setStep(step - 1)}>
					<ArrowLeft className="mr-2 size-3" />
					Back
				</Button>
				<Button
					disabled={!canContinue}
					onClick={() => {
						if (step === 5) {
							setPage("workspaces");
							setGap("ready");
							setStarted(true);
						} else setStep(step + 1);
					}}
				>
					{step === 5 ? "Start Session" : "Continue"}
					<ArrowRight className="ml-2 size-3" />
				</Button>
			</div>
		</Questionnaire>
	);
	const Progress = (
		<nav aria-label="Setup steps" className="grid gap-1">
			{STEPS.map((label, index) => (
				<Button
					key={label}
					variant="ghost"
					className="justify-start"
					disabled={index > step}
					aria-current={index === step ? "step" : undefined}
					onClick={() => setStep(index)}
				>
					{index < step ? <Check className="size-3" /> : <Circle className="size-3" />}
					<span className={index === step ? "font-medium" : "text-muted-foreground"}>{label}</span>
				</Button>
			))}
		</nav>
	);

	return (
		<div className="min-h-dvh pb-36 font-sans">
			<header className="flex flex-wrap items-center justify-between gap-3 border-b px-5 py-3">
				<span className="font-medium tracking-tight">
					kestrel{" "}
					<span className="ml-2 text-xs font-normal text-muted-foreground">
						{page === "setup" ? "Let’s start your first Session" : `${name || "Jack"}’s install`}
					</span>
				</span>
				<nav className="flex flex-wrap gap-1" aria-label="Client navigation">
					<Button variant="ghost" onClick={() => setPage("workspaces")}>
						Workspaces
					</Button>
					{(["signins", "agents", "repositories"] as Page[]).map((one) => (
						<Button
							key={one}
							variant={page === one ? "secondary" : "ghost"}
							onClick={() => setPage(one)}
						>
							<Settings className="mr-1 size-3" />
							{one === "signins" ? "Sign-ins" : one === "agents" ? "Agents" : "Repositories"}
						</Button>
					))}
				</nav>
			</header>
			{fixture === "offline" ? (
				<main className="mx-auto grid max-w-xl gap-6 px-6 py-20">
					<h1 ref={heading} tabIndex={-1} className="text-2xl">
						kestrel isn’t running
					</h1>
					<p className="text-muted-foreground">
						The Client loaded, but it can’t reach the control plane. Inspect its status and logs:
					</p>
					<code className="whitespace-pre-wrap border p-4 text-sm">
						docker compose ps{"\n"}docker compose logs kestrel
					</code>
					<p className="text-xs text-muted-foreground">
						Connection check: http://localhost:7719/operator/readiness
					</p>
					<Button className="w-fit" onClick={() => onSearch({ scene: "empty" })}>
						Retry connection
					</Button>
					<p className="text-xs text-muted-foreground">
						Production retries every few seconds. This fixture stays here for review.
					</p>
				</main>
			) : page === "setup" ? (
				variant === "A" ? (
					<main className="mx-auto max-w-lg px-6 py-14">
						<p className="mb-6 text-xs text-muted-foreground">
							Step {step + 1} of 6 · {STEPS[step]}
						</p>
						{Question}
					</main>
				) : variant === "B" ? (
					<main className="mx-auto grid max-w-4xl gap-8 px-6 py-12 md:grid-cols-3">
						<aside>
							<h2 className="mb-4 text-lg">Set up kestrel</h2>
							<div className="hidden md:block">{Progress}</div>
							<details className="md:hidden">
								<summary className="text-sm">
									Step {step + 1} of 6 · {STEPS[step]}
								</summary>
								{Progress}
							</details>
						</aside>
						<section className="max-w-lg md:col-span-2">{Question}</section>
					</main>
				) : (
					<main className="mx-auto max-w-2xl px-6 py-10">
						<Conversation className="max-h-96 min-h-64">
							<ConversationContent className="p-0">
								{STEPS.slice(0, step).map((label, index) => (
									<div key={label} className="mb-4 border-l pl-4">
										<p className="text-sm text-muted-foreground">{label}</p>
										<p className="mt-1 w-fit rounded-lg bg-muted px-3 py-2 text-sm">
											{[name, HARNESS[harness].label, usedSignIn, repository, brief][index]}
										</p>
										<Button variant="link" size="xs" onClick={() => setStep(index)}>
											Edit
										</Button>
									</div>
								))}
								<p className="mb-4 text-xs text-muted-foreground">kestrel · Step {step + 1} of 6</p>
								{Question}
							</ConversationContent>
						</Conversation>
					</main>
				)
			) : (
				<main className="mx-auto max-w-4xl px-6 py-10">
					{page === "workspaces" && (
						<div className="grid gap-6">
							<h1 ref={heading} tabIndex={-1} className="text-2xl outline-none">
								Workspaces
							</h1>
							{gap === "warning" && (
								<Notice title="Your unused Codex sign-in needs attention">
									Claude Code is ready to use.{" "}
									<Button variant="link" onClick={() => setPage("signins")}>
										Review sign-ins
									</Button>
								</Notice>
							)}
							{gap === "blocked" && (
								<Notice title="This Session’s Claude sign-in needs attention">
									<p>
										The previous Session returned authentication_required. Expiry was not
										established.
									</p>
									<Button
										className="mt-3"
										onClick={() => {
											setPage("signins");
											setRelay("idle");
										}}
									>
										Sign in again
									</Button>
								</Notice>
							)}
							{gap === "image" && (
								<Notice title="Claude Code isn’t available in this image">
									<p>
										<code>ghcr.io/example/custom-env:dev</code> declares only OpenCode. Choose an
										available Agent.
									</p>
									<Button className="mt-3" onClick={() => setPage("agents")}>
										Choose Agent
									</Button>
								</Notice>
							)}
							{gap === "project" && (
								<Notice title="Add a repository to start work">
									<Button className="mt-3" onClick={() => setPage("repositories")}>
										Add repository
									</Button>
								</Notice>
							)}
							{gap === "github" && (
								<Notice title="GitHub access needs attention">
									This Integration’s installation is no longer accessible.{" "}
									<Button variant="link" onClick={() => setPage("repositories")}>
										Review GitHub connection
									</Button>
								</Notice>
							)}
							{readOnly && (
								<Notice title="You can start, but can’t push">
									Connect GitHub to let the Session open a pull request.{" "}
									<Button variant="link" onClick={() => setPage("repositories")}>
										Connect GitHub
									</Button>
								</Notice>
							)}
							{fixture === "resumed" && (
								<Notice title="Continue setting up">
									Your name is saved.{" "}
									<Button variant="link" onClick={() => setPage("setup")}>
										Continue setup
									</Button>
								</Notice>
							)}
							{started ? (
								<div className="grid gap-4">
									<div className="flex flex-wrap justify-between border-y py-3 text-sm">
										<strong>{repository || "openkestrel/kestrel"}</strong>
										<span className="text-muted-foreground">
											Session queued · {HARNESS[harness].label}
										</span>
									</div>
									<p className="w-fit rounded-lg bg-muted p-3 text-sm">
										{brief || "Explore this repository"}
									</p>
									<p className="text-sm text-muted-foreground">
										Your Session is queued. The production Client opens this Workspace’s workbench.
									</p>
									<a
										className="text-sm underline"
										href="/prototype/workbench?variant=D&font=geist&mono=geist"
									>
										View the settled workbench prototype
									</a>
								</div>
							) : (
								<div className="grid gap-4 py-6">
									<h2 className="text-lg">Your first Workspace starts with a Brief</h2>
									<p className="text-sm text-muted-foreground">
										{repository || "Choose a repository"} · {usedSignIn}
									</p>
									<label htmlFor="workspace-brief" className="text-sm">
										Brief
									</label>
									<textarea
										id="workspace-brief"
										className="min-h-28 border bg-background p-3 text-sm"
										value={brief}
										onChange={(event) => setBrief(event.target.value)}
										placeholder="What would you like to work on?"
									/>
									<Button
										className="w-fit"
										disabled={isBlocked || !brief.trim()}
										onClick={() => setStarted(true)}
									>
										Start Session
									</Button>
								</div>
							)}
						</div>
					)}
					{page === "signins" && (
						<div className="grid gap-6">
							<h1 ref={heading} tabIndex={-1} className="text-2xl outline-none">
								Sign-ins
							</h1>
							<p className="text-sm text-muted-foreground">
								Subscriptions belong to you. API keys belong to the Organization. Secret values are
								never shown.
							</p>
							<div className="divide-y border-y">
								{HARNESSES.map((one) => (
									<div className="flex flex-wrap items-center justify-between gap-3 py-3" key={one}>
										<div>
											<strong className="text-sm">{HARNESS[one].label}</strong>
											<p className="mt-1 text-xs text-muted-foreground">
												{one === "claude"
													? `${name || "Jack"}’s Subscription Profile`
													: "No usable sign-in"}
											</p>
										</div>
										<span className="text-xs">
											{one === "claude"
												? gap === "blocked"
													? "Authentication failed · just now"
													: "Signed in · checked 2 minutes ago"
												: gap === "warning" && one === "codex"
													? "Needs attention · unused"
													: "Not signed in"}
										</span>
										<Button
											variant="outline"
											onClick={() => {
												chooseHarness(one);
											}}
										>
											Sign in
										</Button>
									</div>
								))}
							</div>
							<h2 className="text-lg">{HARNESS[harness].label}</h2>
							{SignIn}
							{relay === "success" && (
								<Button className="w-fit" onClick={repairSignIn}>
									Return to work
								</Button>
							)}
						</div>
					)}
					{page === "agents" && (
						<div className="grid gap-6">
							<h1 ref={heading} tabIndex={-1} className="text-2xl outline-none">
								Agents
							</h1>
							<div className="border-y py-4">
								<p className="mb-4 text-sm">Default Agent · {HARNESS[harness].label}</p>
								<label htmlFor="harness" className="mr-3 text-sm">
									Harness
								</label>
								<select
									id="harness"
									className="border bg-background p-2 text-sm"
									value={harness}
									onChange={(event) =>
										chooseHarness(HARNESSES.find((one) => one === event.target.value) ?? "claude")
									}
								>
									{HARNESSES.map((one) => (
										<option key={one} value={one} disabled={gap === "image" && one !== "opencode"}>
											{HARNESS[one].label}
											{gap === "image" && one !== "opencode" ? " · unavailable" : ""}
										</option>
									))}
								</select>
								<div className="mt-4 flex items-center gap-3 text-sm">Model {Model}</div>
								<p className="mt-4 text-xs text-muted-foreground">
									{gap === "image"
										? "Custom image carries only OpenCode. Sign in to OpenCode to use it."
										: "Default image carries Claude Code, Codex and OpenCode."}
								</p>
							</div>
							<Button
								className="w-fit"
								onClick={() => {
									setPage("signins");
									setRelay("idle");
								}}
							>
								Choose sign-in for this Agent
							</Button>
						</div>
					)}
					{page === "repositories" && (
						<div className="grid gap-6">
							<h1 ref={heading} tabIndex={-1} className="text-2xl outline-none">
								Repositories
							</h1>
							{repository && (
								<div className="flex flex-wrap justify-between gap-3 border-y py-3 text-sm">
									<strong>{repository}</strong>
									<span>Default branch · main</span>
									<span>{readOnly ? "Read-only" : "GitHub App"}</span>
								</div>
							)}
							{Repository}
							<Button
								className="w-fit"
								disabled={!repository}
								onClick={() => {
									setGap("ready");
									setPage("workspaces");
								}}
							>
								Use repository
							</Button>
						</div>
					)}
				</main>
			)}
			{import.meta.env.DEV && (
				<div className="fixed inset-x-0 bottom-16 z-40 mx-auto max-w-3xl border bg-background/95 px-3 py-2 shadow-lg">
					<div className="flex flex-wrap items-center gap-2 text-xs">
						<label htmlFor="fixture">PROTOTYPE · fixture</label>
						<select
							id="fixture"
							className="min-w-0 flex-1 border bg-background p-1"
							value={fixture}
							onChange={(event) => onSearch({ scene: event.target.value })}
						>
							{Object.entries(SCENES).map(([value, label]) => (
								<option key={value} value={value}>
									{label}
								</option>
							))}
						</select>
						<Button
							size="xs"
							variant="ghost"
							onClick={() => onSearch({ scene: fixture === "empty" ? "resumed" : "empty" })}
						>
							Reset
						</Button>
						<details className="w-full">
							<summary>Current state (no secrets)</summary>
							<pre className="max-h-24 overflow-auto text-xs">
								{JSON.stringify(
									{
										page,
										step: STEPS[step],
										operator: name,
										harness,
										method,
										relay,
										signedIn,
										repository,
										readOnly,
										model,
										gap,
										started,
									},
									null,
									2,
								)}
							</pre>
						</details>
					</div>
				</div>
			)}
			<SetupSwitcher variant={variant} name={VARIANTS[variant]} theme={theme} onSearch={onSearch} />
		</div>
	);
}

function Field({
	label,
	value,
	onChange,
	placeholder,
	secret,
}: {
	label: string;
	value: string;
	onChange: (next: string) => void;
	placeholder?: string;
	secret?: boolean;
}) {
	return (
		<label className="grid gap-2 text-sm">
			{label}
			<Input
				type={secret ? "password" : "text"}
				autoComplete="off"
				value={value}
				placeholder={placeholder}
				onChange={(event) => onChange(event.target.value)}
			/>
		</label>
	);
}
function Notice({ title, children }: { title: string; children: ReactNode }) {
	return (
		<section className="border-l-2 border-foreground/40 bg-muted/40 p-4 text-sm" aria-label={title}>
			<h2 className="mb-2 font-medium">{title}</h2>
			<div className="text-muted-foreground">{children}</div>
		</section>
	);
}
