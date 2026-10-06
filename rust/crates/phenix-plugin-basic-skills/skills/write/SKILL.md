---
name: write
description: Writing documents for any audience. Use when creating or editing skills, AGENTS.md, CLAUDE.md, or any text the agent or human consumes. Must always apply.
---

Pick the mode that matches your audience. Three modes, each adding constraints to the foundation. The foundation applies to all modes.

## Process

1. Pick the mode. Humans, humans + agents, or agents only.
2. Scan for the foundation patterns. Rewrite.
3. Scan for the mode-specific patterns. Rewrite.
4. Self-audit: "What makes this obviously AI generated?" Fix remaining tells.

## Style foundation

These rules apply in all modes.

### Sentences

1. **Shorten or split dense sentences.** If the reader backtracks to parse it, break it. One idea per sentence. [unslop]
2. **Active voice.** Name the actor. "The compiler validates queries" beats "queries are validated." [unslop]
3. **Cut adverbs, use a stronger verb.** "Runs quickly" becomes "is fast" or the number. An adverb propping a weak verb means the verb is wrong. [unslop]
4. **Prefer the plain word.** "Utilize" becomes "use." "Leverage" becomes "use." "Facilitate" becomes "help." The fancier synonym is rarely clearer. [unslop]

### Content

5. **Cut puffery.** "Pivotal moment," "testament to," "evolving landscape." State what happened. [unslop]
6. **No AI vocabulary.** Additionally, crucial, delve, enduring, enhance, fostering, garner, interplay, intricate, landscape (abstract), pivotal, showcase, tapestry (abstract), testament, underscore, vibrant. Replace with plain words. [unslop]
7. **Fancy ways to say "is."** "Serves as," "stands as," "boasts," "features." Just say "is" or "has." [unslop]
8. **No vague attributions.** "Experts believe," "industry reports suggest." Name the source or delete. [unslop]
9. **Say what it does, not how it feels.** "The database stays close at hand" names a feeling. Write the mechanism: "`.toSQL()` returns the exact string." If you cannot restate it as a concrete instruction, fact, or number, cut it. [unslop]

### Filler

10. **Cut filler phrases.** "In order to" becomes "to." "Due to the fact that" becomes "because." "It is important to note that" gets deleted. [unslop]
11. **Cut excessive hedging.** "Could potentially possibly be argued that it might" becomes "may." [unslop]
12. **No generic conclusions.** "The future looks bright." State specific plans or facts. [unslop]

### Jargon

13. **No abstract metaphor nouns.** Substrate, wedge, vector, locus, nexus, primitive (as noun), harness (as metaphor), surface ("API surface"), bedrock, scaffolding (as metaphor), modality, paradigm, gold-plating, ratchet (as metaphor), evacuate, endgame, north star, flywheel. These read as technical but have a plainer concrete word. [unslop]

### Document structure

14. **Single source of truth.** One authoritative place for each meaning. Duplication costs maintenance and inflates prominence past its real rank. [writing-for-agents]
15. **Prune no-ops.** An instruction the model already obeys pays load to say nothing. The test is whether it changes behaviour versus the default. When a sentence fails, delete the whole sentence. [writing-for-agents]
16. **Check relevance.** Does the line still bear on what the document does? A line loses relevance by never bearing on the task or by going stale. Without pruning, stale layers settle because adding feels safe and removing feels risky. [writing-for-agents]
17. **Sentence case headings.** No title case. [unslop]
18. **No decorative emojis.** Remove from headings and bullets. [unslop]
19. **Straight quotes.** Replace curly quotes. [unslop]

### Data representation

20. **Pick the right form.** When content is data, comparisons, steps, or structured info, use the markdown form that fits: tables for multi-attribute comparisons, lists for ordered or unordered items, code blocks for literal values. Don't describe in prose what a structure already shows. One sentence after may state what's notable or what to derive. [new]

## Mode 1: humans

Prose for human readers. Simple, easy to read, dense in plain language. Complexity lives in the ideas, not in the tokens.

### Token constraints

21. **No em dashes.** Use periods or commas. Em dashes are an AI tell. [unslop]
22. **No parentheses.** If a thought needs separation, end the sentence or use a comma. [unslop]
23. **Colons only before lists or examples.** Not as mid-sentence connectors. "Coming from traditional automation: instead of registering handlers, you describe conditions" adds nothing with the colon. Rewrite to let the point stand. [unslop]
24. **No bold overuse.** Don't bold every proper noun or acronym. A bold lead-in that ends in a period, names the item, and is followed by genuinely new detail is fine. "**Schema in TypeScript.** Tables live in one file." [unslop]
25. **No inline-header lists.** The tell is a bold label and colon that restates the line: "**Performance:** Performance improved..." Convert to prose. [unslop]

### Voice constraints

26. **Have opinions.** React to facts instead of neutrally listing pros and cons. [unslop]
27. **Vary rhythm.** Short sentences. Then longer ones that take their time. Mix it up. [unslop]
28. **Acknowledge complexity.** "Impressive but also kind of unsettling" beats "impressive." [unslop]
29. **Use "I" when it fits.** First person isn't unprofessional. [unslop]
30. **Let some mess in.** Perfect structure looks machine-made. [unslop]
31. **Be specific.** Not "this is concerning" but "there's something unsettling about agents churning away at 3am." [unslop]

### Pattern constraints

32. **No "not just X, but Y."** State the point directly. [unslop]
33. **No rule of three forcing.** Use the natural number. [unslop]
34. **No synonym cycling.** Protagonist, main character, central figure, hero all in one paragraph. Pick one, repeat it. [unslop]
35. **No false ranges.** "From X to Y" where X and Y aren't on a meaningful scale. List topics directly. [unslop]

## Mode 2: humans + agents

For documents both humans and agents read. Skills, AGENTS.md, CLAUDE.md. Slightly denser through sparse use of `;` and `()`, mainly for short, trivial clarifications. Structural tokens add clarity when they earn their place. They don't earn it by existing.

### Allowed structural tokens

36. **Sparse parentheses OK.** For short, trivial clarifications. Keep them brief. If the aside runs long, break it into a sentence. [writing-for-agents]
37. **Sparse semicolons OK.** Join closely related independent clauses. Keep both clauses short. If either is complex, use a period. [new]
38. **Colons as structural markers OK.** Introduce definitions, rules, or structured content. Not comparison framing mid-sentence. [writing-for-agents]
39. **Bold for leading words.** A leading word is a compact concept the agent thinks with. Bold it on first use. Reuse as a token after that. _Context pointer_. _Completion criterion_. _Leading word_. [writing-for-agents]

### Document design

40. **Context pointers.** A reference held in context that names out-of-context material and encodes when to reach it. The pointer's wording, not its target, decides when the agent reaches the material. Sharpen the wording first. Inline only if sharpening fails. [writing-for-agents]
41. **Information hierarchy.** Steps are ordered actions. Reference is definitions, rules, facts. Push reference behind pointers so steps stay legible. Inline what every branch needs. Push behind a pointer what only some branches reach. [writing-for-agents]
42. **Progressive disclosure.** Move material out of the main file and behind a pointer. Not primarily token optimization. It protects the hierarchy. [writing-for-agents]
43. **Co-location.** Keep a concept's definition, rules, and caveats under one heading. Grouped material reads like documentation written for the agent. Scattered material does not. [writing-for-agents]
44. **Leading words.** A compact concept already in the model's pretraining. Repeated as a token, never as a sentence, it accumulates a distributed definition across the document. Reach for an existing word before coining one. A made-up word recruits no priors. [writing-for-agents]
45. **No negation steering.** Prohibition drags the forbidden behaviour into context and makes it more available. State the target behaviour so the banned one is never spoken. A prohibition earns its place only as a hard guardrail you cannot phrase positively. Pair it with the positive target. [writing-for-agents]

## Mode 3: agents only

Maximum density. Use any tokens that increase information density. Everything is allowed as long as it keeps things short.

46. **All semantic tokens allowed.** Em dashes, colons, semicolons, parentheses, brackets, arrows, symbols. Use anything that adds semantics or density. [new]
47. **Maximum compression.** Abbreviations, acronyms, shorthand OK. The agent can expand them. [new]
48. **No prose overhead.** Skip transitions, hedging, qualifiers. State the fact. [new]
49. **Structured data preferred.** Tables, lists, key-value pairs over prose when the content fits. [new]
50. **Environment as source of truth.** `package.json` scripts, config files, directory layout, `--help` output. A document that restates the environment is a cache, earning its load only when the lookup is expensive. Cache what the agent cannot find by looking. The unwritten convention. The reason behind a choice. The gotcha no config confesses. [writing-for-agents]
51. **Telegraphic style.** Grammar is optional. Fragments, shorthand, semantic compression are fine. "Problem: api duplicates for <endpoint>" beats "The API has a problem — its interface for <endpoint> has a duplicate implementation." Every word earns its place by carrying meaning. [new]
