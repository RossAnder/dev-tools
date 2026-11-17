Verdict: A vision model alone is not a reliable gate for fine-grained layout defects (overflow, clipping and overlap sit near chance in published benchmarks, and it errs towards a false pass), so the success criterion at docs/flows/v2-ui-fidelity/brief.md:32 needs DOM geometry checks for those classes. The image critic should be a separate, skeptical agent that works through an enumerated defect rubric over zoomed crops, with the chosen mock beside the capture and the capture taken at content and viewport extremes. A before-and-after pair adds little.

No code in this repo decides this question. The answer rests on published evaluations and one practitioner report, all fetched on 2026-11-17.

**Answer**

- **What the model can and cannot judge.** On coarse, semantic judgements, frontier models do moderately well: whether an element is undisplayed, whether text is ellipsised, whether a control looks out of place, whether there is a large empty band. Their F1 runs from about 57% to 81%. On boundary-precise defects (text overflow, cropped content, overlapping containers), accuracy averages about 50% to 54% on a balanced set, which is chance. The best single model reaches about 65% F1. Run without help, the model will miss most of the defects that the brief's success criterion names "clipped or overflowing".
- **Which way it fails.** For this gate the failure is a false pass, not a false alarm. On before-and-after pairs, Claude Opus 4.7 reported a difference that did not exist on only 0.4% of the pairs that had none, yet found only 31% of the real changes. Anthropic's own harness report says a Claude evaluator "talk[s] itself into deciding they weren't a big deal" and is "inclined to be generous towards LLM-generated outputs".
- **What reduces false passes, ranked by the strength of the evidence:**
  1. **Do not leave measurable classes to the image.** Overflow, clipping, overlap and an element wider than its container can all be computed from DOM geometry (element boxes, `scrollWidth` against `clientWidth`, collisions). ReDeCheck found 91% of injected layout faults this way. Run those checks in the capture step and give their output to the critic as evidence. This crosses into the screenshot-capture and accept-look lenses.
  2. **Use a separate critic, made skeptical, with a defect checklist and worked examples.** The harness report found a standalone evaluator "far more tractable" to tune than generator self-critique, and calibrated it with few-shot score breakdowns. UICrit reports a 55% gain in critique quality from few-shot examples plus visual grounding. Have the critic answer each defect class from the brief as a separate yes-or-no question with a location, not give one overall score, and fail the state whenever the answer is unsure.
  3. **Zoom.** Models struggle with visual grounding. A refinement pass that re-inspects a zoomed patch around each candidate region helps, and low-level geometric perception is weak even for the best models.
  4. **Content and viewport extremes.** Capturing across viewport widths exposes failures without needing an oracle. ReDeCheck's consistency check found failures in 16 of 26 production pages, and split or folded viewports raised the number of defects exposed by 137% to 196%. Dead space, a panel that scales with its container and a fixed-height panel only show up at the extremes: an empty list, a very long label, a narrow viewport and a wide one.
  5. **A chosen mock beside the capture.** No controlled study isolates this, so treat it as a hypothesis. The Design2Code human protocol judges a page against a reference in the order content, then layout, then style. An AWS evaluation found that a reference improves faithfulness judgements but distracts the judge from checking structure. So the mock answers "is this the layout the user picked", and the defect checklist should run as its own pass, not merged with the comparison.
  6. **Before-and-after pairs are weak.** The best model found 40.7% of single-property CSS changes, and none exceeded 23% recall on the hard tier. Use pairs only to flag regressions for a closer look, never as proof that nothing changed.

**Findings**

- **Library/API or file**: UI-Lens, CVPR 2026 (Xiang et al.), the benchmark of six UI display-defect categories
- **Source**: https://cvpr.thecvf.com/virtual/2026/poster/38861 (abstract) and https://en.papernotes.org/CVPR2026/multimodal_vlm/ui-lens_assessing_general_mllms_potential_to_automate_ui_display_quality_assuran/ (tables), both fetched 2026-11-17
- **Evidence-grade**: medium. The abstract is primary; the per-category table comes from a secondary summary, because the paper PDF returned 403.
- **Finding**: General multimodal models are near chance on text overflow, cropped content and container overlap, and moderate on undisplayed content and ellipsis. An image-only critic will pass most clipping and overflow defects.
- **Details**: The abstract gives an average F1 of 20.36% for text overflow and 31.21% for container overlap across 10 models. The summary gives a mean F1 of 42.05% for cropped content, 57.78% for undisplayed content and 75.24% for abnormal ellipsis, and boundary-task accuracy of 50% to 54% on a 1:1 balanced set. Gemini 2.5 Pro reaches the best text-overflow F1 at 64.67%, with recall 81% and precision 57%. One-shot prompting, self-correction and visual prompting each added at most about 2.5 points. The data is Chinese-language mobile apps, and the guidelines exclude clipping of scrollable elements.
- **Counter**: A benchmark of desktop web UI with Claude-generation models in which overflow and clipping F1 exceeds about 85% would overturn this. UI-Lens tested no Claude model in the summarised table, and it covers mobile pages.
- **Impact on plan / report**: The gate's checks for overflow, clipping and overlap should rest on DOM geometry, with the image critic second. The image critic owns dead space, panel sizing, off-style controls and fidelity to the mock.

- **Library/API or file**: DiffSpot, arXiv 2605.29615, the benchmark of before-and-after web-page pairs
- **Source**: https://arxiv.org/html/2605.29615, fetched 2026-11-17
- **Evidence-grade**: high (primary paper)
- **Finding**: On before-and-after pairs, frontier models catch a minority of real layout changes and almost never invent one. The pair form therefore biases towards a false pass.
- **Details**: The benchmark has 4,400 pairs, each with one mutated CSS property, and tests 13 models zero-shot. The best recall is 40.7% (Gemini 3.1 Pro). Claude Opus 4.7 reaches 31.2% recall, falling to 21.8% on the hard tier, and reports a non-existent difference on 0.4% of the no-difference pairs. `line_height` changes are found with a median recall of 4%. Difficulty depends on which property changed, not on how many pixels changed. The paper did not test side-by-side images, overlays or pixel-diff hints.
- **Counter**: A study showing that a highlighted overlay or a pixel-diff mask lifts recall above about 80% would make pairs a strong form. That form is untested here.
- **Impact on plan / report**: Do not offer "matches the previous capture" as evidence of no defect. Pairs are at most a regression flag.

- **Library/API or file**: Anthropic Engineering, "Harness design for long-running application development"
- **Source**: https://www.anthropic.com/engineering/harness-design-long-running-apps, fetched 2026-11-17
- **Evidence-grade**: high (first-party practitioner report; not a controlled evaluation)
- **Finding**: Claude self-grades design leniently, and even a standalone evaluator rationalises away the defects it found. A separate critic tuned for skepticism and calibrated with few-shot examples is the practical fix.
- **Details**: Quotes from the report: agents "reliably skew positive when grading their own work", "particularly pronounced for subjective tasks like design". The evaluator would "talk itself into deciding they weren't a big deal". Also "out of the box, Claude is a poor QA agent". The evaluator drove Playwright, screenshotting and studying the page, against four criteria, and was calibrated "using few-shot examples with detailed score breakdowns".
- **Counter**: A measured result that worker self-check catches as many visual defects as an independent critic would weaken this. The report gives no numbers either way.
- **Impact on plan / report**: Bears on the `critic-placement` decision. Worker self-checks can stay as a cheap pre-filter, but the gate needs an independent critic, and its prompt should forbid downgrading a defect it has observed: "observed means fail or report".

- **Library/API or file**: UICrit (Duan et al., UIST 2024, arXiv 2407.08850), and "Visual Prompting with Iterative Refinement" (arXiv 2412.16829)
- **Source**: https://arxiv.org/abs/2407.08850 and https://arxiv.org/html/2412.16829v2, via search abstracts fetched 2026-11-17
- **Evidence-grade**: medium (abstract-level reading)
- **Finding**: Few-shot examples of critiques, chosen for visual similarity, together with visual grounding aids (coordinate marks on the edge, zoomed patches around candidate regions) measurably improve LLM UI critique.
- **Details**: UICrit is 3,059 designer critiques of 983 mobile UIs. Few-shot plus visual prompting gave a 55% gain in feedback quality. The follow-up adds a zoomed-patch refinement step because "MLLMs still struggle with accurate visual grounding".
- **Counter**: Gains measured on critique quality against designers may not carry over to the recall of specific defects. Neither paper reports recall for overflow or clipping.
- **Impact on plan / report**: The rubric ships worked examples (a defect image and its verdict) for each defect class, and the critic re-inspects crops of each region it flags or is asked about.

- **Library/API or file**: ReDeCheck (Walsh, Kapfhammer and McMinn, ASE 2015 and ISSTA 2017)
- **Source**: https://www.cs.allegheny.edu/sites/gkapfham/research/papers/Walsh2015 and https://www.cs.allegheny.edu/sites/gkapfham/research/papers/Walsh2017, via search abstracts fetched 2026-11-17
- **Evidence-grade**: medium (abstracts)
- **Finding**: DOM-geometry checks across a range of viewport widths find layout failures deterministically and need no reference image.
- **Details**: It found 91% of injected layout faults, and its oracle-free consistency check across viewport widths found 33 failures in 16 of 26 production pages. It can report structural failures that are not visible, so it needs triage. The multi-window GUI defect study (arXiv 2604.19081) supports varying the viewport: split and folded windows exposed 137% to 196% more overlap, truncation and occlusion defects.
- **Counter**: If the projects' capture harness (`@vitest/browser`) cannot reach element geometry in the same run, this route costs a second harness. The screenshot-capture lens settles that.
- **Impact on plan / report**: The capture contract should declare content extremes and at least two viewports for each state, and emit geometry assertions next to the PNGs.

- **Library/API or file**: "MLLM as a UI Judge", arXiv 2510.08783, and WebDevJudge (ICLR 2026, arXiv 2510.18560)
- **Source**: https://arxiv.org/html/2510.08783v1 fetched 2026-11-17; https://arxiv.org/pdf/2510.18560 via search abstract
- **Evidence-grade**: medium
- **Finding**: Holistic scores and pairwise preference agree only loosely with humans, so a single overall "looks good" score is the wrong verdict form.
- **Details**: Claude 3.5 Sonnet was within ±1 point of human ratings 77% of the time, with Pearson 0.69. Its pairwise accuracy was 60% overall and 47% on Ease of Use, rising to about 93% only when the human score gap was large. The top WebDevJudge evaluator agrees with experts 70% of the time, pairwise.
- **Counter**: These studies measure aesthetic and usability perception, not defects, so they bound the holistic form but say nothing about a checklist form.
- **Impact on plan / report**: The verdict should be per defect class and per state, with a location, never a single quality score.

Out of lens (one line): the image resolution the Read tool hands the critic, and whether it downsamples a full-page capture, decides how well zooming works. That is the screenshot-capture lens's question.

Coverage: read docs/flows/v2-ui-fidelity/brief.md in full. No tree code bears on this lens, and no commands decide it. Ruled out: Owl Eyes and Nighthawk-style trained detectors, because they need a fine-tuned model, which the harness cannot ship; the absolute-rating form, which loses to a checklist on the evidence above.
Searched: queries on MLLM detection of UI display defects (overflow, clipping), "MLLM as a UI Judge", UICrit and visual grounding, "VLMs are blind", Anthropic's harness evaluator, VLM before-and-after visual regression, reference-based against reference-free MLLM judging and Design2Code, and ReDeCheck. Sources: UI-Lens CVPR 2026 poster page and papernotes summary; DiffSpot arXiv 2605.29615; arXiv 2510.08783; the Anthropic engineering post; arXiv 2407.08850 and 2412.16829; arXiv 2604.19081; ReDeCheck abstracts; the AWS Strands Evals blog (search snippet only); all fetched 2026-11-17. The newest source is DiffSpot (2026-05). Dead ends: the UI-Lens PDF (403) and an arXiv preprint for it (none found). No controlled study of a mock beside the capture against no mock was found, nor of side-by-side images against a diff overlay.

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = ["docs/flows/v2-ui-fidelity/brief.md"]

[[source]]
path = "docs/flows/v2-ui-fidelity/brief.md"
line = 32
claim = "Success criterion naming the defect classes the critic must catch"

[[source]]
url = "https://cvpr.thecvf.com/virtual/2026/poster/38861"
claim = "UI-Lens: MLLMs near random on text overflow (F1 20.36) and container overlap (F1 31.21)"

[[source]]
url = "https://en.papernotes.org/CVPR2026/multimodal_vlm/ui-lens_assessing_general_mllms_potential_to_automate_ui_display_quality_assuran/"
claim = "UI-Lens per-category F1 table, precision/recall skews, small prompting gains"

[[source]]
url = "https://arxiv.org/html/2605.29615"
claim = "DiffSpot: best before/after recall 40.7%, Claude Opus 4.7 31.2% recall with 0.4% hallucination"

[[source]]
url = "https://www.anthropic.com/engineering/harness-design-long-running-apps"
claim = "Self-grading skews positive on design; standalone skeptical evaluator with few-shot calibration; evaluator talks itself out of issues"

[[source]]
url = "https://arxiv.org/abs/2407.08850"
claim = "UICrit: few-shot plus visual prompting gives 55% gain in LLM UI feedback"

[[source]]
url = "https://arxiv.org/html/2412.16829v2"
claim = "Zoomed-patch refinement for visual grounding of UI critiques"

[[source]]
url = "https://arxiv.org/html/2604.19081v1"
claim = "Split/fold viewports expose 137-196% more overlap, truncation and occlusion defects"

[[source]]
url = "https://www.cs.allegheny.edu/sites/gkapfham/research/papers/Walsh2015"
claim = "ReDeCheck DOM-geometry detection found 91% of injected layout faults"

[[source]]
url = "https://www.cs.allegheny.edu/sites/gkapfham/research/papers/Walsh2017"
claim = "Oracle-free consistency checks across viewport widths found failures in 16 of 26 production pages"

[[source]]
url = "https://arxiv.org/html/2510.08783v1"
claim = "Holistic and pairwise MLLM UI judgements only loosely match humans"

[[source]]
url = "https://arxiv.org/pdf/2510.18560"
claim = "WebDevJudge: best evaluator reaches 70.34% expert agreement pairwise"
```
