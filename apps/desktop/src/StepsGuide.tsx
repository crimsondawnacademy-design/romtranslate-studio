import { makeT, MessageId } from "./i18n";

export type StepState = "done" | "todo" | "error" | "running";

export interface Step {
  key: string;
  title: MessageId;
  state: StepState;
  /** O que falta (ou o que ja foi feito). */
  detail: string;
  /** O id do painel do passo: e la que o erro dele aparece. */
  panel: string;
  panelName: string;
}

/**
 * O processo inteiro na ordem, do arquivo ao patch: cada passo diz o que
 * falta e em que painel o erro dele aparece, e leva ate o painel.
 */
export default function StepsGuide({
  steps,
  t,
}: {
  steps: Step[];
  t: ReturnType<typeof makeT>;
}) {
  return (
    <nav className="steps" aria-label={t("steps.title")}>
      <h3>{t("steps.title")}</h3>
      <ol>
        {steps.map((step, i) => (
          <li key={step.key} className={`step ${step.state}`}>
            <button
              className="step-go"
              onClick={() =>
                document
                  .getElementById(step.panel)
                  ?.scrollIntoView({ behavior: "smooth", block: "start" })
              }
            >
              <span className="step-n">{i + 1}</span>
              {t(step.title)}
              <span className={`step-state ${step.state}`}>
                {t(`steps.state.${step.state}` as MessageId)}
              </span>
            </button>
            <p>{step.detail}</p>
            <p className="step-where">
              {step.state === "error"
                ? t("steps.errorAt", { panel: step.panelName })
                : t("steps.where", { panel: step.panelName })}
            </p>
          </li>
        ))}
      </ol>
    </nav>
  );
}
