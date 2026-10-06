---
name: to-questionnaire
description: Turn an unanswered decision into a questionnaire for someone else. Use when the user needs input from another person to move forward.
---

Turn something the user cannot answer alone into a **questionnaire**: a Markdown document they hand to one person to fill in async or together over a meeting. The recipient holds knowledge the user lacks. The questionnaire pulls it out.

## Process

**Grill the send, not the subject.** Interview the user only about the send, which they can always answer: who it goes to and what they need back. The questions in the document target the **gap** between what the recipient knows and what the user needs.

1. **Who is it going to?** Ask, in one exchange, the recipient's role, expertise, and relationship to the user. This fixes the tone and how much context the document must carry. Done when you know who the recipient is and what they know that the user does not.

2. **What do you need back?** Ask, in one exchange, the specific decisions or facts the user cannot resolve alone. Done when you have a concrete list of what the user must walk away able to decide.

3. **Write the questionnaire.** Draft questions aimed at the gap from steps 1 and 2. Follow the template below. Write to `to-questionnaire-<slug>.md` in the current directory. Report the path. Done when the file exists and every item from step 2 is covered by a question.

## Questionnaire template

Frame the document as a **discovery questionnaire**: the user lacks context, the recipient holds it. Order questions most-important-first. Async means you may only get one pass. Group under `##` headings by theme once there are more than a handful.

---

# <questionnaire title>

**Purpose:** why this questionnaire exists and the decision riding on it.

**From:** <the user>, **To:** <the recipient>, **How your answers will be used:** <where they go>

## Context

One paragraph orienting a recipient who was not in the user's head. Enough to answer well, not a page.

## How to answer

Deadline and rough effort. Partial answers and "I don't know" are useful. Flag anything you are unsure of rather than skipping it.

## <theme heading>

One `##` section per theme. Under each, its questions, most-important-first. Each question covers one idea and stands alone. An answer stub sits directly beneath, with a one-line reason to ask only where the question could be misread.

### What load is the system expected to handle at launch?

_Why this matters: it decides whether we provision for burst traffic now or defer it._

> [answer here]

## Anything else?

A closing catch-all for anything we did not ask.

---
