import { useEffect, useRef, type KeyboardEvent } from "react";

interface Props {
  folderName: string;
  destinationName: string;
  busy: boolean;
  error: string | null;
  onMoveContents: () => void;
  onDeleteContents: () => void;
  onCancel: () => void;
}

export function FolderDeleteDialog({ folderName, destinationName, busy, error, onMoveContents, onDeleteContents, onCancel }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const moveRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!dialogRef.current?.open) dialogRef.current?.showModal();
    cancelRef.current?.focus();
  }, []);

  function handleKeyDown(event: KeyboardEvent<HTMLDialogElement>) {
    if (event.key !== "Tab" || busy) return;
    if (event.shiftKey && document.activeElement === moveRef.current) {
      event.preventDefault();
      cancelRef.current?.focus();
    } else if (!event.shiftKey && document.activeElement === cancelRef.current) {
      event.preventDefault();
      moveRef.current?.focus();
    }
  }

  return (
    <dialog
      ref={dialogRef}
      className="confirm-dialog folder-delete-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="folder-delete-title"
      aria-describedby="folder-delete-description"
      aria-busy={busy}
      onKeyDown={handleKeyDown}
      onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }}
    >
      <h2 id="folder-delete-title">Delete “{folderName}”?</h2>
      <p id="folder-delete-description">Choose what happens to the media and subfolders inside.</p>
      <div className="folder-delete-choices">
        <button ref={moveRef} type="button" onClick={onMoveContents} disabled={busy}>
          <strong>DELETE FOLDER ONLY</strong>
          <span>Move everything inside to “{destinationName}”.</span>
        </button>
        <button className="folder-delete-everything" type="button" onClick={onDeleteContents} disabled={busy}>
          <strong>DELETE FOLDER &amp; CONTENTS</strong>
          <span>Permanently delete its media files and subfolders.</span>
        </button>
      </div>
      {error && <p className="form-error" role="alert">{error}</p>}
      <div className="confirm-actions">
        <button ref={cancelRef} className="secondary-button" type="button" onClick={onCancel} disabled={busy}>CANCEL</button>
      </div>
    </dialog>
  );
}
