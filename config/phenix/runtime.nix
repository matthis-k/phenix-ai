let
  # Transitional callable contract: the current conductor invokes agents and
  # sequential orchestrations with one non-empty textual objective and emits textual
  # assistant content. Do not model these as generic structured objects until
  # OrchestrationDefinition has real typed value/binding dataflow.
  inputSchema = {
    type = "string";
    minLength = 1;
  };
  outputSchema = {
    type = "string";
  };

  descriptor = kind: id: description: {
    inherit id kind description;
    input_schema = inputSchema;
    output_schema = outputSchema;
    capabilities = [ ];
    policy.requires_permission = false;
  };

  agents = {
    coordinator =
      descriptor "agent" "agent.coordinator"
        "Coordinate bounded specialist work and preserve explicit decisions without editing the codebase.";
    scout =
      descriptor "agent" "agent.scout"
        "Inspect concrete repository and ecosystem evidence without changing code.";
    planner =
      descriptor "agent" "agent.planner"
        "Convert settled intent into an executable plan without implementing it.";
    architect =
      descriptor "agent" "agent.architect"
        "Model boundaries, ownership, invariants, and architectural tradeoffs without implementing them.";
    implementer =
      descriptor "agent" "agent.implementer"
        "Own bounded code, test, and instrumentation mutations needed to satisfy the settled objective.";
    tester =
      descriptor "agent" "agent.tester"
        "Act as a read-only evidence analyst: run existing feedback, reproduce behavior, and distinguish observations from hypotheses without editing files.";
    critic =
      descriptor "agent" "agent.critic"
        "Challenge engineering quality, architecture, and evidence independently without editing the codebase.";
    verifier =
      descriptor "agent" "agent.verifier"
        "Verify conformance to requested behavior and acceptance evidence independently without editing the codebase.";
    finalizer =
      descriptor "agent" "agent.finalizer"
        "Synthesize established evidence without inventing new findings or editing implementation files.";
    qa_synthesizer =
      descriptor "agent" "agent.qa-synthesizer"
        "Synthesize independent QA evidence while preserving provenance and disagreement without editing implementation files.";
  };

  node = agent: task: {
    callable = agents.${agent}.id;
    objective = "${agents.${agent}.description} ${task} for the current orchestration objective.";
  };

  orchestration = id: title: nodes: {
    descriptor = descriptor "orchestration" id title;
    policy = "sequential";
    inherit nodes;
  };

  qaReviewNodes = [
    (node "coordinator" "Coordinate independent QA branches and keep their evidence separate")
    (node "scout" "Review repository structure, correctness, duplication, and integration seams")
    (node "tester" "Run or interpret deterministic checks and identify coverage gaps")
    (node "architect" "Review architecture, module boundaries, ownership, and migration containment")
    (node "critic" "Review trust boundaries, unsafe assumptions, and concrete security risks")
    (node "qa_synthesizer" "Produce one prioritized QA report from the independent review evidence")
  ];

  orchestrations = [
    (orchestration "orchestration.implement" "Phenix implementation" [
      (node "planner" "Produce the minimum executable plan appropriate to the requested change")
      (node "implementer" "Apply the plan using existing abstractions and keep the change bounded")
      (node "verifier" "Independently verify requested behavior, deterministic checks, and relevant regressions")
    ])

    (orchestration "orchestration.qa" "Phenix QA" qaReviewNodes)

    (orchestration "orchestration.qa-fix" "Phenix QA and fix" (
      qaReviewNodes
      ++ [
        (node "planner" "Turn actionable QA findings into a bounded repair plan and keep the plan empty when there is nothing to fix")
        (node "implementer" "Apply only actionable repairs justified by the QA report")
        (node "verifier" "Verify repaired findings independently and guard against regressions")
        (node "finalizer" "Produce the final QA-and-fix handoff with evidence and unresolved findings")
      ]
    ))

    (orchestration "orchestration.design" "Phenix design" [
      (node "scout" "Inspect requirements, constraints, and reusable mechanisms")
      (node "planner" "Develop viable alternatives and an executable decision plan")
      (node "architect" "Evaluate ownership, interfaces, data flow, and invariants")
      (node "critic" "Challenge assumptions, tradeoffs, and failure modes")
      (node "finalizer" "Produce the decision-oriented design handoff")
    ])

    (orchestration "orchestration.migrate" "Phenix migration" [
      (node "scout" "Inventory contracts, providers, consumers, and compatibility surfaces affected by the migration")
      (node "planner" "Produce an ordered migration plan with explicit expand-migrate-contract boundaries where required")
      (node "implementer" "Execute the migration and remove superseded paths when consumers have moved")
      (node "critic" "Audit migrated consumers, obsolete interfaces, duplication, and migration containment")
      (node "finalizer" "Produce the migration handoff and remaining follow-ups")
    ])

    (orchestration "orchestration.refactor" "Phenix refactor" [
      (node "scout" "Capture public behavior, invariants, existing tests, and dependency shape before structural change")
      (node "architect" "Define intended ownership, module boundaries, and dependency direction")
      (node "implementer" "Apply the behavior-preserving refactor and remove obsolete duplication")
      (node "critic" "Review architecture, semantic preservation, and whether the refactor actually reduces complexity")
      (node "finalizer" "Produce the refactor handoff with behavioral evidence")
    ])

    (orchestration "orchestration.security" "Phenix security review" [
      (node "scout" "Map entry points, assets, privilege boundaries, and externally controlled data")
      (node "architect" "Model ownership, trust boundaries, abuse paths, and security invariants")
      (node "critic" "Validate concrete security risks adversarially and reject speculative findings without evidence")
      (node "finalizer" "Produce the evidence-backed security handoff")
    ])

    (orchestration "orchestration.ui-change" "Phenix UI change" [
      (node "scout" "Inspect interaction, rendering, focus, input, and state-update paths")
      (node "architect" "Specify layout, focus, input, feedback, and update invariants before implementation")
      (node "implementer" "Implement the UI change using the highest appropriate existing UI abstractions")
      (node "tester" "Exercise existing framework-appropriate UI scenarios and interaction regressions")
      (node "critic" "Review interaction quality, predictability, hierarchy, and state consistency")
      (node "finalizer" "Produce the UI change handoff with scenario evidence")
    ])

    (orchestration "orchestration.debug" "Evidence-driven diagnosis" [
      (node "tester" "Reproduce the failure with the narrowest existing feedback path and capture the exact observable evidence")
      (node "tester" "Derive the smallest reproducing scenario without editing production or test files")
      (node "critic" "Rank falsifiable root-cause hypotheses and state the evidence that would discriminate them")
      (node "tester" "Design the narrowest diagnostic experiment or instrumentation needed to discriminate the leading hypotheses")
      (node "implementer" "Add only the bounded diagnostic instrumentation or harness change required by the experiment")
      (node "tester" "Run the diagnostic experiment and determine which hypotheses the resulting evidence supports or rejects")
      (node "implementer" "Remove temporary diagnostics as appropriate and apply the smallest root-cause repair justified by the evidence")
      (node "verifier" "Re-run the reproducer and relevant regressions and verify durable regression coverage exists where appropriate")
      (node "finalizer" "Summarize reproduction, causal evidence, repair, and residual uncertainty")
    ])

    (orchestration "orchestration.review" "Independent code review" [
      (node "critic" "Review only engineering quality, architecture, maintainability, correctness risks, tests, and duplication")
      (node "verifier" "Review only conformance to the stated request, specification, and acceptance criteria")
    ])

    (orchestration "orchestration.research" "Source-oriented research" [
      (node "scout" "Investigate repository code, tests, documentation, and history")
      (node "scout" "Investigate authoritative upstream documentation, specifications, releases, and prior art")
      (node "scout" "Investigate risks, constraints, edge cases, and counterexamples")
      (node "critic" "Challenge contradictions, source quality, unsupported conclusions, and missing counterevidence")
      (node "finalizer" "Produce a source-oriented handoff separating facts, inferences, disagreement, and uncertainty")
    ])

    (orchestration "orchestration.grill" "Alignment grilling" [
      (node "scout" "Resolve questions already answered by code, tests, documentation, and existing decisions")
      (node "coordinator" "Stress-test one unresolved decision at a time and keep prerequisite decisions ordered")
      (node "architect" "Normalize settled vocabulary and identify only durable architectural decisions")
      (node "finalizer" "Produce the durable decision and context handoff without implementing the feature")
    ])

    (orchestration "orchestration.spec" "Specification synthesis" [
      (node "scout" "Recover settled intent, project vocabulary, constraints, and existing behavior")
      (node "architect" "Identify the highest stable implementation and acceptance seams")
      (node "planner" "Write an implementation-independent specification with invariants, acceptance criteria, and non-goals")
      (node "verifier" "Verify the specification is grounded, testable, and free of invented product decisions")
    ])

    (orchestration "orchestration.tickets" "Tracer-bullet decomposition" [
      (node "architect" "Identify prerequisite structural changes that make the implementation easy before making the easy change")
      (node "planner" "Decompose work into independently verifiable vertical slices with explicit blocking edges")
      (node "critic" "Reject needless fragmentation, oversized tickets, and horizontal slicing unless migration requires it")
      (node "finalizer" "Produce the blocker-first ticket frontier with acceptance evidence and dependencies")
    ])

    (orchestration "orchestration.tdd" "Test-driven development" [
      (node "tester" "Identify the smallest durable test seam, expected assertion, and intended failing reason without editing files")
      (node "implementer" "Add the focused regression test and prove it fails for the intended missing behavior before changing production behavior")
      (node "implementer" "Make the smallest coherent production change that turns the focused test green without weakening the test")
      (node "implementer" "Improve boundaries and remove duplication while keeping focused feedback green")
      (node "verifier" "Run focused and surrounding validation and verify requested behavior without regression")
    ])

    (orchestration "orchestration.architecture" "Architecture deepening" [
      (node "scout" "Map concrete code paths, abstractions, duplicated knowledge, naming, and dependency direction")
      (node "architect" "Find opportunities to deepen modules, reduce exposed concepts, and reuse stronger existing abstractions")
      (node "critic" "Challenge migration cost, accidental abstraction, coupling, and whether the proposal actually simplifies the system")
      (node "planner" "Produce a prioritized architecture plan with tradeoffs, migration containment, and validation seams")
    ])

    (orchestration "orchestration.domain-model" "Domain modeling" [
      (node "scout" "Collect terminology, entities, operations, invariants, and contradictory names from code and documentation")
      (node "architect" "Choose canonical terms and define their semantic boundaries and relationships")
      (node "critic" "Reject aliases, overloaded terms, and abstractions that do not reduce conceptual ambiguity")
      (node "finalizer" "Produce settled domain vocabulary and unresolved semantic conflicts")
    ])

    (orchestration "orchestration.wayfinder" "Long-horizon wayfinding" [
      (node "scout" "Identify constraints, unknowns, irreversible decisions, dependencies, and available evidence")
      (node "planner" "Build a compact decision and investigation map and identify the current frontier")
      (node "architect" "Resolve the highest-leverage architecture and domain decisions that evidence can settle")
      (node "verifier" "Verify the frontier names remaining uncertainty and is ready for specification and decomposition")
    ])
  ];

  target = provider: model: effort: {
    backend = "phenix";
    inherit provider model;
    inference = { inherit effort; };
  };

  roles = [
    "coordinator"
    "scout"
    "planner"
    "architect"
    "implementer"
    "tester"
    "verifier"
    "critic"
    "finalizer"
    "qa_synthesizer"
  ];

  routingProfile = id: targets: {
    inherit id;
    default_target = targets.fallback;
    callable_targets = builtins.listToAttrs (
      map (role: {
        name = agents.${role}.id;
        value = targets.${role};
      }) roles
    );
  };

  mixedTargets = {
    coordinator = target "openai-codex" "gpt-5.6-terra" "medium";
    scout = target "opencode-go" "mimo-v2.5" "low";
    planner = target "openai-codex" "gpt-5.6-terra" "medium";
    architect = target "openai-codex" "gpt-5.6-terra" "high";
    implementer = target "opencode-go" "qwen3.7-plus" "medium";
    tester = target "opencode-go" "deepseek-v4-flash" "medium";
    verifier = target "openai-codex" "gpt-5.6-terra" "high";
    critic = target "openai-codex" "gpt-5.6-terra" "high";
    finalizer = target "opencode-go" "qwen3.7-plus" "medium";
    qa_synthesizer = target "openai-codex" "gpt-5.6-terra" "high";
    fallback = target "opencode-go" "qwen3.7-plus" "medium";
  };

  openaiApiTargets = {
    coordinator = target "openai-api" "gpt-5.6-terra" "medium";
    scout = target "openai-api" "gpt-5.6-luna" "low";
    planner = target "openai-api" "gpt-5.6-terra" "medium";
    architect = target "openai-api" "gpt-5.6-terra" "high";
    implementer = target "openai-api" "gpt-5.6-sol" "medium";
    tester = target "openai-api" "gpt-5.6-luna" "medium";
    verifier = target "openai-api" "gpt-5.6-terra" "high";
    critic = target "openai-api" "gpt-5.6-terra" "high";
    finalizer = target "openai-api" "gpt-5.6-sol" "medium";
    qa_synthesizer = target "openai-api" "gpt-5.6-terra" "high";
    fallback = target "openai-api" "gpt-5.6-sol" "medium";
  };

  opencodeGoTargets = {
    coordinator = target "opencode-go" "gpt-5.6-luna" "medium";
    scout = target "opencode-go" "mimo-v2.5" "low";
    planner = target "opencode-go" "qwen3.7-plus" "medium";
    architect = target "opencode-go" "qwen3.7-plus" "high";
    implementer = target "opencode-go" "qwen3.7-plus" "medium";
    tester = target "opencode-go" "mimo-v2.5" "medium";
    verifier = target "opencode-go" "qwen3.7-plus" "high";
    critic = target "opencode-go" "deepseek-v4-flash" "high";
    finalizer = target "opencode-go" "qwen3.7-plus" "medium";
    qa_synthesizer = target "opencode-go" "qwen3.7-plus" "high";
    fallback = target "opencode-go" "qwen3.7-plus" "medium";
  };

  chatgptPlusTargets = {
    coordinator = target "openai-codex" "gpt-5.6-terra" "medium";
    scout = target "openai-codex" "gpt-5.6-luna" "low";
    planner = target "openai-codex" "gpt-5.6-terra" "medium";
    architect = target "openai-codex" "gpt-5.6-terra" "high";
    implementer = target "openai-codex" "gpt-5.6-sol" "medium";
    tester = target "openai-codex" "gpt-5.6-luna" "medium";
    verifier = target "openai-codex" "gpt-5.6-terra" "high";
    critic = target "openai-codex" "gpt-5.6-terra" "high";
    finalizer = target "openai-codex" "gpt-5.6-sol" "medium";
    qa_synthesizer = target "openai-codex" "gpt-5.6-terra" "high";
    fallback = target "openai-codex" "gpt-5.6-sol" "medium";
  };

  freeTarget = target "opencode-zen" "mimo-v2.5-free" "medium";
  freeTargets = builtins.listToAttrs (
    map (name: {
      inherit name;
      value = freeTarget;
    }) (roles ++ [ "fallback" ])
  );
in
{
  agents = builtins.attrValues agents;
  inherit orchestrations;
  routing_profiles = [
    (routingProfile "default" openaiApiTargets)
    (routingProfile "router.mixed" mixedTargets)
    (routingProfile "router.openai-api" openaiApiTargets)
    (routingProfile "router.opencode-go" opencodeGoTargets)
    (routingProfile "router.chatgpt-plus" chatgptPlusTargets)
    (routingProfile "router.free" freeTargets)
  ];
}