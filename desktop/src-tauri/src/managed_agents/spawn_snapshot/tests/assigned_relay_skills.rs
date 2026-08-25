use super::*;

#[test]
fn restart_stamp_uses_live_shared_instructions_and_clears_diff() {
    let mut rec = record();
    rec.persona_id = Some("pers".into());
    rec.assigned_relay_skills = vec![
        format!("30023:{}:kept", "b".repeat(64)),
        format!("30023:{}:revoked", "a".repeat(64)),
    ];
    let mut live = persona("pers", Some("goose"), "prompt");
    live.assigned_relay_skills = vec![format!("30023:{}:kept", "b".repeat(64))];
    let personas = [live];
    let global = GlobalAgentConfig::default();
    let descriptor =
        crate::managed_agents::resolve_effective_harness_descriptor(&rec, &personas, &global)
            .unwrap();
    let effective = resolve_effective_config(&rec, &personas, &global)
        .require_resolved()
        .unwrap();
    let mut command = std::process::Command::new("buzz-acp");
    let assigned =
        crate::managed_agents::relay_skills::resolve_and_apply_assigned_relay_skills_env(
            &mut command,
            &rec,
            &personas,
        )
        .unwrap();
    let stamped = SpawnConfigSnapshot::from_inputs(SpawnConfigInputs {
        record: &rec,
        descriptor: &descriptor,
        relay_url: "wss://ws.example",
        team_instructions: None,
        system_prompt: effective.system_prompt.value.as_deref(),
        model: effective.model.value.as_deref(),
        provider: effective.provider.value.as_deref(),
        assigned_relay_skills: assigned,
        enforced_owner_only: false,
    });
    let current =
        prospective_spawn_config_snapshot(&rec, &personas, &[], "wss://ws.example", &global, false);

    assert_eq!(
        stamped.assigned_relay_skills,
        personas[0].assigned_relay_skills
    );
    assert_eq!(stamped.canonical(), current.canonical());
}
