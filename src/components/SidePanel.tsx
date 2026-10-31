import { useEffect, useRef, type ReactNode } from "react";

interface SidePanelProps {
  title: string;
  eyebrow?: string;
  open: boolean;
  onClose: () => void;
  onCancel?: () => void;
  className?: string;
  children: ReactNode;
}

export function SidePanel({ title, eyebrow = "BITIG", open, onClose, onCancel, className = "", children }: SidePanelProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    if (open && !ref.current?.open) ref.current?.showModal();
    if (!open && ref.current?.open) ref.current.close();
  }, [open]);

  return (
    <dialog ref={ref} className={`side-panel ${className}`} onCancel={(event) => { event.preventDefault(); (onCancel ?? onClose)(); }} onClose={onClose}>
      <div className="side-panel-frame">
        <div className="side-panel-heading">
          <div><p className="eyebrow">{eyebrow}</p><h2>{title}</h2></div>
          <button className="close-button" type="button" onClick={onClose} aria-label="Close panel">×</button>
        </div>
        {children}
      </div>
    </dialog>
  );
}
