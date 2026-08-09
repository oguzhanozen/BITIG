import { useEffect, useRef, type ReactNode } from "react";

interface SidePanelProps {
  title: string;
  eyebrow?: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

export function SidePanel({ title, eyebrow = "BITIG", open, onClose, children }: SidePanelProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    if (open && !ref.current?.open) ref.current?.showModal();
    if (!open && ref.current?.open) ref.current.close();
  }, [open]);

  return (
    <dialog ref={ref} className="side-panel" onCancel={onClose} onClose={onClose}>
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
