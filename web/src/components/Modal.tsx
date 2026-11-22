import { AnimatePresence, motion } from "motion/react";
import { useEffect, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface Props {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  actions?: ReactNode;
  wide?: boolean;
}

export function Modal({ open, onClose, title, children, actions, wide }: Props) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="modal modal-open modal-bottom sm:modal-middle"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          role="dialog"
          aria-modal
        >
          <motion.div
            className={`modal-box ${wide ? "sm:max-w-2xl" : ""}`}
            initial={{ y: 24, scale: 0.97 }}
            animate={{ y: 0, scale: 1 }}
            exit={{ y: 24, scale: 0.97 }}
            transition={{ type: "spring", stiffness: 400, damping: 32 }}
          >
            <h3 className="mb-4 text-lg font-bold">{title}</h3>
            {children}
            {actions && <div className="modal-action">{actions}</div>}
          </motion.div>
          <div className="modal-backdrop" onClick={onClose} />
        </motion.div>
      )}
    </AnimatePresence>
  );
}

export function ConfirmModal({
  open,
  onClose,
  onConfirm,
  title,
  message,
  confirmLabel,
  busy,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: ReactNode;
  confirmLabel?: string;
  busy?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={title}
      actions={
        <>
          <button className="btn btn-ghost" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button className="btn btn-error" onClick={onConfirm} disabled={busy}>
            {busy && <span className="loading loading-spinner loading-sm" />}
            {confirmLabel ?? t("common.delete")}
          </button>
        </>
      }
    >
      <div className="text-base-content/80">{message}</div>
    </Modal>
  );
}
