import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";

export interface AppSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface AppSelectProps {
  value: string;
  options: readonly AppSelectOption[];
  onChange: (value: string) => void;
  ariaLabel: string;
  disabled?: boolean;
  id?: string;
}

interface ListPosition {
  top: number;
  left: number;
  width: number;
  maxHeight: number;
}

export function AppSelect({ value, options, onChange, ariaLabel, disabled = false, id }: AppSelectProps) {
  const listId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const typeaheadRef = useRef({ query: "", at: 0 });
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [position, setPosition] = useState<ListPosition | null>(null);
  const [portalTarget, setPortalTarget] = useState<Element | null>(null);
  const selectedIndex = options.findIndex((option) => option.value === value);
  const selectedOption = options[selectedIndex];
  const isOpen = open && !disabled;

  function firstEnabledIndex() {
    return options.findIndex((option) => !option.disabled);
  }

  function lastEnabledIndex() {
    for (let index = options.length - 1; index >= 0; index -= 1) {
      if (!options[index]?.disabled) return index;
    }
    return -1;
  }

  function openList(preferredIndex = selectedIndex) {
    if (disabled || options.every((option) => option.disabled)) return;
    setPortalTarget(rootRef.current?.closest("dialog") ?? document.body);
    setPosition(null);
    setActiveIndex(preferredIndex >= 0 && !options[preferredIndex]?.disabled ? preferredIndex : firstEnabledIndex());
    setOpen(true);
  }

  function selectOption(index: number) {
    const option = options[index];
    if (!option || option.disabled) return;
    setOpen(false);
    onChange(option.value);
    triggerRef.current?.focus();
  }

  function stepIndex(direction: 1 | -1) {
    if (!options.length) return -1;
    let index = activeIndex;
    for (let count = 0; count < options.length; count += 1) {
      index = (index + direction + options.length) % options.length;
      if (!options[index]?.disabled) return index;
    }
    return -1;
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (disabled) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      if (isOpen) setActiveIndex(stepIndex(direction));
      else openList(direction === 1 ? firstEnabledIndex() : lastEnabledIndex());
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      const index = event.key === "Home" ? firstEnabledIndex() : lastEnabledIndex();
      if (isOpen) setActiveIndex(index);
      else openList(index);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (isOpen) selectOption(activeIndex);
      else openList();
    } else if (event.key === "Escape" && isOpen) {
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
    } else if (event.key === "Tab") {
      setOpen(false);
    } else if (event.key.length === 1 && !event.altKey && !event.ctrlKey && !event.metaKey) {
      const now = Date.now();
      const previous = typeaheadRef.current;
      const query = (now - previous.at < 700 ? previous.query : "") + event.key.toLocaleLowerCase();
      typeaheadRef.current = { query, at: now };
      const match = options.findIndex((option) => !option.disabled && option.label.toLocaleLowerCase().startsWith(query));
      if (match >= 0) {
        event.preventDefault();
        if (isOpen) setActiveIndex(match);
        else openList(match);
      }
    }
  }

  useLayoutEffect(() => {
    if (!isOpen) return;
    const updatePosition = () => {
      const rect = triggerRef.current?.getBoundingClientRect();
      if (!rect) return;
      const width = Math.min(Math.max(rect.width, 150), window.innerWidth - 16);
      const left = Math.max(8, Math.min(rect.left, window.innerWidth - width - 8));
      const wantedHeight = Math.min(260, options.length * 36 + 8);
      const below = window.innerHeight - rect.bottom - 8;
      const above = rect.top - 8;
      const showAbove = below < wantedHeight && above > below;
      const maxHeight = Math.max(36, Math.min(wantedHeight, (showAbove ? above : below) - 4));
      setPosition({ top: showAbove ? rect.top - maxHeight - 4 : rect.bottom + 4, left, width, maxHeight });
    };
    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [isOpen, options.length]);

  useEffect(() => {
    if (!isOpen) return;
    const closeOutside = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (!rootRef.current?.contains(target) && !listRef.current?.contains(target)) setOpen(false);
    };
    document.addEventListener("pointerdown", closeOutside);
    return () => document.removeEventListener("pointerdown", closeOutside);
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen || activeIndex < 0) return;
    document.getElementById(`${listId}-option-${activeIndex}`)?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, listId, isOpen, position]);

  return (
    <div className="app-select" ref={rootRef}>
      <button
        ref={triggerRef}
        id={id}
        className="app-select-trigger"
        type="button"
        role="combobox"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-controls={isOpen ? listId : undefined}
        aria-activedescendant={isOpen && activeIndex >= 0 ? `${listId}-option-${activeIndex}` : undefined}
        disabled={disabled}
        onClick={() => { if (isOpen) setOpen(false); else openList(); }}
        onKeyDown={handleKeyDown}
      >
        <span className="app-select-value">{selectedOption?.label ?? "Select…"}</span>
        <span className="app-select-chevron" aria-hidden="true" />
      </button>
      {isOpen && position && portalTarget && createPortal(
        <div
          ref={listRef}
          id={listId}
          className="app-select-list"
          role="listbox"
          aria-label={ariaLabel}
          style={position}
          onPointerDown={(event) => event.stopPropagation()}
        >
          {options.map((option, index) => (
            <div
              id={`${listId}-option-${index}`}
              key={option.value}
              className={`app-select-option${index === activeIndex ? " is-active" : ""}${option.value === value ? " is-selected" : ""}${option.disabled ? " is-disabled" : ""}`}
              role="option"
              aria-selected={option.value === value}
              aria-disabled={option.disabled || undefined}
              onPointerEnter={() => { if (!option.disabled) setActiveIndex(index); }}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => selectOption(index)}
            >
              {option.label}
            </div>
          ))}
        </div>,
        portalTarget,
      )}
    </div>
  );
}
