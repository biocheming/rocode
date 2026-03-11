// ── Question Panel ─────────────────────────────────────────────────────────

function humanQuestionStatusLabel(status) {
  switch (String(status || "").toLowerCase()) {
    case "pending":
      return "Awaiting Answer";
    case "answered":
      return "Answered";
    case "rejected":
      return "Rejected";
    case "cancelled":
      return "Cancelled";
    case "error":
      return "Error";
    default:
      return "Question";
  }
}

function questionStatusTone(status) {
  const normalized = String(status || "").toLowerCase();
  if (normalized === "answered") return "done";
  if (normalized === "rejected" || normalized === "cancelled" || normalized === "error") return "error";
  return "waiting";
}

function normalizeQuestionAnswer(input, options, multiple) {
  const raw = String(input || "").trim();
  if (!raw) return [];
  const values = multiple ? raw.split(",") : [raw];
  return values
    .map((value) => {
      const trimmed = value.trim();
      if (!trimmed) return null;
      const index = Number.parseInt(trimmed, 10);
      if (!Number.isNaN(index) && index > 0 && index <= options.length) {
        return options[index - 1];
      }
      return trimmed;
    })
    .filter(Boolean);
}

async function answerQuestionInteraction(interaction) {
  openQuestionPanel(interaction);
}

async function rejectQuestionInteraction(interaction) {
  if (!interaction || !interaction.request_id) return;
  await api(`/question/${interaction.request_id}/reject`, {
    method: "POST",
    body: JSON.stringify({}),
  });
}

function openQuestionPanel(interaction) {
  if (!interaction || !interaction.request_id) return;
  state.activeQuestionInteraction = interaction;
  state.questionSubmitting = false;
  renderQuestionPanel();
  nodes.questionPanel.classList.remove("hidden");
  const firstInput = nodes.questionList.querySelector("input, textarea");
  if (firstInput) firstInput.focus();
}

function closeQuestionPanel() {
  if (state.questionSubmitting) return;
  nodes.questionPanel.classList.add("hidden");
  state.activeQuestionInteraction = null;
  nodes.questionList.replaceChildren();
}

function renderQuestionPanel() {
  const interaction = state.activeQuestionInteraction;
  nodes.questionList.replaceChildren();
  if (!interaction) return;

  const questions = Array.isArray(interaction.questions) ? interaction.questions : [];
  nodes.questionPanelTitle.textContent = questions.length > 1 ? "Answer Questions" : "Answer Question";
  nodes.questionPanelStatus.textContent = humanQuestionStatusLabel(interaction.status);
  nodes.questionPanelMeta.textContent = `${questions.length} item${questions.length === 1 ? "" : "s"} · ${interaction.request_id}`;
  nodes.questionRejectBtn.disabled = state.questionSubmitting;
  nodes.questionSubmitBtn.disabled = state.questionSubmitting;
  nodes.questionSubmitBtn.textContent = state.questionSubmitting ? "Submitting..." : "Submit Answers";

  questions.forEach((question, index) => {
    const card = document.createElement("section");
    card.className = "question-item";

    const header = document.createElement("div");
    header.className = "question-item-header";

    const label = document.createElement("div");
    label.className = "question-item-label";
    label.textContent = question.header || `Question ${index + 1}`;
    header.appendChild(label);

    const mode = document.createElement("span");
    mode.className = "stage-chip";
    mode.textContent = question.multiple ? "multi-select" : "single-select";
    header.appendChild(mode);
    card.appendChild(header);

    const text = document.createElement("div");
    text.className = "question-item-text";
    text.textContent = question.question || "";
    card.appendChild(text);

    const options = Array.isArray(question.options) ? question.options : [];
    if (options.length > 0) {
      const optionsWrap = document.createElement("div");
      optionsWrap.className = "question-options";
      options.forEach((option, optionIndex) => {
        const optionLabel = document.createElement("label");
        optionLabel.className = "question-option";

        const input = document.createElement("input");
        input.type = question.multiple ? "checkbox" : "radio";
        input.name = `question-option-${index}`;
        input.value = option;
        input.dataset.questionIndex = String(index);
        input.dataset.optionIndex = String(optionIndex);
        optionLabel.appendChild(input);

        const textWrap = document.createElement("span");
        textWrap.className = "question-option-text";
        textWrap.textContent = option;
        optionLabel.appendChild(textWrap);

        optionsWrap.appendChild(optionLabel);
      });
      card.appendChild(optionsWrap);
    }

    const customWrap = document.createElement("div");
    customWrap.className = "question-custom";

    const customLabel = document.createElement("label");
    customLabel.className = "question-custom-label";
    customLabel.setAttribute("for", `question-custom-${index}`);
    customLabel.textContent = options.length > 0 ? "Other" : "Answer";
    customWrap.appendChild(customLabel);

    const customInput = options.length > 0 ? document.createElement("textarea") : document.createElement("textarea");
    customInput.id = `question-custom-${index}`;
    customInput.className = "question-custom-input";
    customInput.rows = options.length > 0 ? 2 : 3;
    customInput.dataset.questionIndex = String(index);
    customInput.placeholder = options.length > 0
      ? "Type your own answer if none of the options fit"
      : "Type your answer";
    customWrap.appendChild(customInput);

    card.appendChild(customWrap);
    nodes.questionList.appendChild(card);
  });
}

function collectQuestionPanelAnswers() {
  const interaction = state.activeQuestionInteraction;
  if (!interaction) return [];
  const questions = Array.isArray(interaction.questions) ? interaction.questions : [];
  return questions.map((question, index) => {
    const selected = Array.from(
      nodes.questionList.querySelectorAll(`input[name="question-option-${index}"]:checked`)
    ).map((input) => input.value);
    const customInput = nodes.questionList.querySelector(`#question-custom-${index}`);
    const custom = customInput ? String(customInput.value || "").trim() : "";
    if (custom) selected.push(custom);
    return selected;
  });
}

function interactionFromLiveQuestionEvent(payload) {
  const questions = Array.isArray(payload.questions) ? payload.questions : [];
  return {
    type: "question",
    status: "pending",
    request_id: payload.requestID || payload.requestId,
    can_reply: true,
    can_reject: true,
    questions: questions.map((question) => ({
      question: question.question || "",
      header: question.header || null,
      multiple: Boolean(question.multiple),
      options: Array.isArray(question.options)
        ? question.options.map((option) => {
            if (typeof option === "string") return option;
            return option && option.label ? option.label : "";
          }).filter(Boolean)
        : [],
    })),
  };
}

function renderQuestionInteraction(entry, interaction) {
  entry.interactionNode.replaceChildren();
  if (!interaction || interaction.type !== "question") {
    entry.interactionNode.classList.add("hidden");
    return;
  }
  entry.interactionNode.classList.remove("hidden");

  const statusChip = document.createElement("span");
  statusChip.className = `stage-chip status-chip ${questionStatusTone(interaction.status)}`;
  statusChip.textContent = humanQuestionStatusLabel(interaction.status);
  entry.interactionNode.appendChild(statusChip);

  if (interaction.status === "pending" && interaction.request_id) {
    const actions = document.createElement("div");
    actions.className = "question-action-buttons";

    const answerBtn = document.createElement("button");
    answerBtn.className = "question-action-btn primary";
    answerBtn.type = "button";
    answerBtn.textContent = "Answer";
    answerBtn.addEventListener("click", async () => {
      answerQuestionInteraction(interaction);
    });

    const rejectBtn = document.createElement("button");
    rejectBtn.className = "question-action-btn";
    rejectBtn.type = "button";
    rejectBtn.textContent = "Reject";
    rejectBtn.addEventListener("click", async () => {
      answerBtn.disabled = true;
      rejectBtn.disabled = true;
      try {
        await rejectQuestionInteraction(interaction);
        await loadMessages();
      } catch (error) {
        applyOutputBlock({
          kind: "status",
          tone: "error",
          text: `Failed to reject question: ${String(error)}`,
        });
      } finally {
        answerBtn.disabled = false;
        rejectBtn.disabled = false;
      }
    });

    actions.appendChild(answerBtn);
    actions.appendChild(rejectBtn);
    entry.interactionNode.appendChild(actions);
  }
}
