use jiff::Timestamp;
use kestrel::declaration::{triggers, triggers::Applied};
use kestrel::domain::{
    CorrelationMiss, Declared, EventRecordId, Fires, Occurrence, OnOpenWorkspace, Schedule,
    Templates, Trigger,
};
use kestrel::trigger::{self, Against, Asked, Tested};

use super::super::{
    BRIEF, Declaration, Kestrel, correlation_overridden, labelled_on, placed_trigger, templates,
};

impl Kestrel {
    pub async fn declare_trigger(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
    ) -> Trigger {
        self.declare_trigger_rendering(
            organization,
            name,
            filter,
            project,
            agent,
            &templates(BRIEF, None, None),
        )
        .await
    }

    pub async fn declare_trigger_declaring(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
        declared: Declared,
    ) -> Trigger {
        placed_trigger(
            &self.store,
            Declaration {
                organization,
                name,
                fires: &Fires::On(filter.parse().expect("the filter should parse")),
                templates: &templates(BRIEF, None, None),
                project,
                agent,
                declared: &declared,
                allows: &[],
                profile: None,
            },
        )
        .await
        .expect("the trigger should declare")
    }

    pub async fn declare_trigger_rendering(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
        templates: &Templates,
    ) -> Trigger {
        self.declare_trigger_rendering_with_miss(
            organization,
            name,
            filter,
            project,
            agent,
            templates,
            templates.correlation.on_miss(),
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn declare_trigger_rendering_with_miss(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
        templates: &Templates,
        on_miss: Option<CorrelationMiss>,
    ) -> Trigger {
        self.try_declare_trigger_rendering_with_miss(
            organization,
            name,
            filter,
            project,
            agent,
            templates,
            on_miss,
        )
        .await
        .expect("the trigger should declare")
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn try_declare_trigger_rendering_with_miss(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
        templates: &Templates,
        on_miss: Option<CorrelationMiss>,
    ) -> anyhow::Result<Trigger> {
        let templates = correlation_overridden(
            templates,
            on_miss,
            templates.correlation.on_open_workspace(),
        )?;
        placed_trigger(
            &self.store,
            Declaration {
                organization,
                name,
                fires: &Fires::On(filter.parse().expect("the filter should parse")),
                templates: &templates,
                project,
                agent,
                declared: &Declared::default(),
                allows: &[],
                profile: None,
            },
        )
        .await
    }

    /// Labelled `ready-for-agent` on the repository, starting its work with `agent` unless a
    /// label chooses one of `allows`.
    pub async fn declare_trigger_allowing(
        &self,
        organization: &str,
        repository: &str,
        agent: &str,
        allows: &[&str],
        correlation: Option<&str>,
    ) -> Trigger {
        placed_trigger(
            &self.store,
            Declaration {
                organization,
                name: "ready",
                fires: &Fires::On(
                    labelled_on(repository, "ready-for-agent")
                        .parse()
                        .expect("the filter should parse"),
                ),
                templates: &templates(BRIEF, None, correlation),
                project: "kestrel",
                agent,
                declared: &Declared::default(),
                allows: &allows
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect::<Vec<_>>(),
                profile: None,
            },
        )
        .await
        .expect("the trigger should declare")
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn declare_correlated_trigger(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        agent: &str,
        allows: &[&str],
        templates: &Templates,
        on_open_workspace: OnOpenWorkspace,
    ) -> Trigger {
        self.try_declare_correlated_trigger(
            organization,
            name,
            filter,
            agent,
            allows,
            templates,
            on_open_workspace,
        )
        .await
        .expect("the trigger should declare")
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn try_declare_correlated_trigger(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        agent: &str,
        allows: &[&str],
        templates: &Templates,
        on_open_workspace: OnOpenWorkspace,
    ) -> anyhow::Result<Trigger> {
        let templates = correlation_overridden(
            templates,
            templates.correlation.on_miss(),
            on_open_workspace,
        )?;
        placed_trigger(
            &self.store,
            Declaration {
                organization,
                name,
                fires: &Fires::On(filter.parse().expect("the filter should parse")),
                templates: &templates,
                project: "kestrel",
                agent,
                declared: &Declared::default(),
                allows: &allows
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect::<Vec<_>>(),
                profile: None,
            },
        )
        .await
    }

    pub async fn apply_triggers(&self, organization: &str, file: &str) -> Applied {
        triggers::apply(
            &self.store,
            organization,
            &triggers::parse(file).expect("the declaration file should parse"),
            false,
        )
        .await
        .expect("the declaration file should apply")
    }

    pub async fn test_trigger(
        &self,
        organization: &str,
        name: &str,
        event: EventRecordId,
    ) -> Tested {
        self.try_test_trigger(organization, name, event)
            .await
            .expect("the trigger should test")
    }

    pub async fn try_test_trigger(
        &self,
        organization: &str,
        name: &str,
        event: EventRecordId,
    ) -> anyhow::Result<Tested> {
        trigger::test(
            &self.store,
            organization,
            name,
            Against::Event(event),
            Asked::default(),
        )
        .await
    }

    pub async fn dispatch(
        &self,
        organization: &str,
        name: &str,
        issue: i64,
        asked: trigger::Asked<'_>,
    ) -> anyhow::Result<trigger::Fired> {
        trigger::dispatch(
            &self.store,
            &kestrel::integration::github::Github::dialling_out(&self.store)?,
            trigger::Dispatch {
                organization,
                trigger: name,
                integration: "github",
                issue,
                asked,
            },
        )
        .await
    }

    pub async fn test_dispatch(
        &self,
        organization: &str,
        name: &str,
        issue: i64,
        asked: Asked<'_>,
    ) -> anyhow::Result<Tested> {
        trigger::test(
            &self.store,
            organization,
            name,
            Against::Issue {
                github: &kestrel::integration::github::Github::dialling_out(&self.store)?,
                integration: "github",
                issue,
            },
            asked,
        )
        .await
    }

    pub async fn try_declare_scheduled_trigger(
        &self,
        organization: &str,
        name: &str,
        schedule: Schedule,
        templates: &Templates,
    ) -> anyhow::Result<Trigger> {
        placed_trigger(
            &self.store,
            Declaration {
                organization,
                name,
                fires: &Fires::Scheduled(schedule),
                templates,
                project: "kestrel",
                agent: "builder",
                declared: &Declared::default(),
                allows: &[],
                profile: None,
            },
        )
        .await
    }

    pub async fn test_scheduled_trigger(&self, organization: &str, name: &str) -> Tested {
        self.try_test_trigger_naming_no_event(organization, name)
            .await
            .expect("the trigger should test against its next elapsing")
    }

    pub async fn try_test_trigger_naming_no_event(
        &self,
        organization: &str,
        name: &str,
    ) -> anyhow::Result<Tested> {
        trigger::test(
            &self.store,
            organization,
            name,
            Against::NextElapsing,
            Asked::default(),
        )
        .await
    }

    /// Stands in for the wheel reaching `at`, which a test cannot wait for.
    pub async fn elapse(&self, at: Timestamp) -> Vec<Occurrence> {
        trigger::elapse(&self.store, at)
            .await
            .expect("the due schedules should elapse")
    }

    pub async fn test_declared_trigger(
        &self,
        organization: &str,
        file: &str,
        name: &str,
        event: EventRecordId,
    ) -> Tested {
        let declarations = triggers::parse(file).expect("the declaration file should parse");
        let declared = declarations
            .iter()
            .find(|declared| declared.name() == name)
            .expect("the declaration file should declare the trigger");

        trigger::test_declared(
            &self.store,
            organization,
            declared,
            Against::Event(event),
            Asked::default(),
        )
        .await
        .expect("the declared trigger should test")
    }

    pub async fn firings(&self, event: EventRecordId) -> Vec<kestrel::domain::Firing> {
        trigger::firings(&self.store, event)
            .await
            .expect("the firings should read")
    }

    pub async fn triggers(&self, organization: &str) -> Vec<Trigger> {
        trigger::triggers(&self.store, organization)
            .await
            .expect("the triggers should list")
    }

    pub async fn show_trigger(&self, organization: &str, name: &str) -> Trigger {
        trigger::show(&self.store, organization, name)
            .await
            .expect("the trigger should show")
    }

    pub async fn disable_trigger(&self, organization: &str, name: &str) -> Trigger {
        trigger::disable(&self.store, organization, name)
            .await
            .expect("the trigger should disable")
    }

    pub async fn enable_trigger(&self, organization: &str, name: &str) -> Trigger {
        trigger::enable(&self.store, organization, name)
            .await
            .expect("the trigger should enable")
    }
}
