import { useEffect, useState } from "react";

const steps = [
  {
    eyebrow: "STEP 1 / 3",
    title: "Bring in a link",
    body: "Paste a supported media URL on the main screen, analyze it, then choose the video or audio version you want to keep.",
  },
  {
    eyebrow: "STEP 2 / 3",
    title: "Keep it organized",
    body: "Create folders from the sidebar. Media can be moved, selected in batches, filtered, or deleted directly from the collection.",
  },
  {
    eyebrow: "STEP 3 / 3",
    title: "Watch the tools",
    body: "Downloads appear automatically while active. Open Tools to check the bundled FFmpeg and yt-dlp versions or restart this tour.",
  },
] as const;

interface QuickTourProps {
  onClose: () => void;
}

export function QuickTour({ onClose }: QuickTourProps) {
  const [step, setStep] = useState(0);
  const current = steps[step];

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  if (!current) return null;

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section className="center-modal quick-tour" role="dialog" aria-modal="true" aria-labelledby="quick-tour-title">
        <div className="modal-heading">
          <div><p className="eyebrow">QUICK TOUR</p><h2 id="quick-tour-title">Getting started</h2></div>
          <button className="popover-close" type="button" onClick={onClose} aria-label="Close quick tour">×</button>
        </div>
        <div className="tour-progress" aria-label={`Step ${step + 1} of ${steps.length}`}>
          {steps.map((entry, index) => <i key={entry.title} className={index <= step ? "is-active" : ""} />)}
        </div>
        <div className="tour-copy">
          <p className="eyebrow">{current.eyebrow}</p>
          <h3>{current.title}</h3>
          <p>{current.body}</p>
        </div>
        <div className="modal-actions">
          <button className="quiet-action" type="button" onClick={onClose}>{step === steps.length - 1 ? "CLOSE" : "SKIP"}</button>
          {step > 0 && <button className="secondary-button" type="button" onClick={() => setStep((value) => value - 1)}>BACK</button>}
          <button className="primary-button" type="button" onClick={() => {
            if (step === steps.length - 1) onClose();
            else setStep((value) => value + 1);
          }}>{step === steps.length - 1 ? "FINISH" : "NEXT"}</button>
        </div>
      </section>
    </div>
  );
}
