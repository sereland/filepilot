import { useEffect, useId, useRef, type ReactNode } from "react";
import { IconX } from "./icons";

interface Props {
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  onClose: () => void;
  busy?: boolean;
  compact?: boolean;
  closeOnBackdrop?: boolean;
}

export default function Modal({ title, description, children, footer, onClose, busy, compact, closeOnBackdrop = true }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const dialog = ref.current!;
    const previousFocus = document.activeElement as HTMLElement | null;
    dialog.showModal();
    dialog.querySelector<HTMLElement>("input, select")?.focus();
    return () => {
      dialog.close();
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, []);
  return (
    <dialog ref={ref} className={`modal${compact ? " compact" : ""}`} aria-labelledby={titleId}
      onCancel={(e) => { e.preventDefault(); if (!busy) onClose(); }}
      onClick={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        if (closeOnBackdrop && !busy && e.target === e.currentTarget && (e.clientX < rect.left || e.clientX > rect.right || e.clientY < rect.top || e.clientY > rect.bottom)) onClose();
      }}>
      <div className="modal-header"><div><h2 id={titleId}>{title}</h2>{description && <p>{description}</p>}</div>
        <button type="button" className="icon-btn" aria-label="关闭" disabled={busy} onClick={onClose}><IconX /></button>
      </div>
      <div className="modal-body">{children}</div>
      {footer && <div className="modal-footer">{footer}</div>}
    </dialog>
  );
}
