import * as React from "react";
import { BookOpen, Pencil } from "lucide-react";

import {
  resolveRelaySkills,
  type RelaySkillCover,
  type ResolvedRelaySkill,
} from "@/shared/api/tauriPersonas";
import {
  DEFAULT_POPOVER_HOVER_OPEN_DELAY_MS,
  Popover,
  PopoverAnchor,
  PopoverContent,
} from "@/shared/ui/popover";
import { PortalledScrollArea } from "@/shared/ui/PortalledScrollArea";
import { Button } from "@/shared/ui/button";

const HOVER_CLOSE_DELAY_MS = 350;

type PreviewOwner = symbol;

const previewClaimListeners = new Set<(owner: PreviewOwner) => void>();

function claimPreview(owner: PreviewOwner) {
  for (const listener of previewClaimListeners) {
    listener(owner);
  }
}

type RelaySkillPreviewButtonProps = Omit<
  React.ButtonHTMLAttributes<HTMLButtonElement>,
  "children"
> & {
  asChild?: boolean;
  children: React.ReactNode;
  onEdit?: (detail: ResolvedRelaySkill) => void;
  previewAlign?: React.ComponentProps<typeof PopoverContent>["align"];
  previewOnFocus?: boolean;
  previewSide?: React.ComponentProps<typeof PopoverContent>["side"];
  skill: RelaySkillCover;
};

type PreviewState = "idle" | "loading" | "ready" | "error";

export function RelaySkillPreviewButton({
  asChild = false,
  children,
  onBlur,
  onClick,
  onEdit,
  onFocus,
  onMouseEnter,
  onMouseLeave,
  previewAlign = "center",
  previewOnFocus = true,
  previewSide = "top",
  skill,
  ...buttonProps
}: RelaySkillPreviewButtonProps) {
  const [open, setOpen] = React.useState(false);
  const [previewState, setPreviewState] = React.useState<PreviewState>("idle");
  const [detail, setDetail] = React.useState<ResolvedRelaySkill | null>(null);
  const contentRef = React.useRef<HTMLDivElement | null>(null);
  const hoverTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const pointerInsidePreviewRef = React.useRef(false);
  const previewOwnerRef = React.useRef<PreviewOwner>(
    Symbol("relay-skill-preview"),
  );
  const triggerRef = React.useRef<HTMLElement | null>(null);

  const clearHoverTimer = React.useCallback(() => {
    if (hoverTimerRef.current !== null) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  }, []);

  const openPreview = React.useCallback(() => {
    claimPreview(previewOwnerRef.current);
    setOpen(true);
  }, []);

  React.useEffect(() => {
    const handlePreviewClaim = (owner: PreviewOwner) => {
      if (owner === previewOwnerRef.current) return;
      clearHoverTimer();
      pointerInsidePreviewRef.current = false;
      setOpen(false);
    };
    previewClaimListeners.add(handlePreviewClaim);
    return () => {
      previewClaimListeners.delete(handlePreviewClaim);
    };
  }, [clearHoverTimer]);

  const openWithDelay = React.useCallback(() => {
    clearHoverTimer();
    hoverTimerRef.current = setTimeout(() => {
      openPreview();
    }, DEFAULT_POPOVER_HOVER_OPEN_DELAY_MS);
  }, [clearHoverTimer, openPreview]);

  const closeWithDelay = React.useCallback(() => {
    clearHoverTimer();
    hoverTimerRef.current = setTimeout(() => {
      if (
        pointerInsidePreviewRef.current ||
        contentRef.current?.matches(":hover") ||
        contentRef.current?.contains(document.activeElement)
      ) {
        return;
      }
      setOpen(false);
    }, HOVER_CLOSE_DELAY_MS);
  }, [clearHoverTimer]);

  React.useEffect(() => clearHoverTimer, [clearHoverTimer]);

  React.useEffect(() => {
    const content = contentRef.current;
    if (!open || !content) return;

    const handlePointerEnter = () => {
      pointerInsidePreviewRef.current = true;
      clearHoverTimer();
    };
    const handlePointerLeave = () => {
      pointerInsidePreviewRef.current = false;
      closeWithDelay();
    };
    content.addEventListener("pointerenter", handlePointerEnter);
    content.addEventListener("pointerleave", handlePointerLeave);
    return () => {
      content.removeEventListener("pointerenter", handlePointerEnter);
      content.removeEventListener("pointerleave", handlePointerLeave);
      pointerInsidePreviewRef.current = false;
    };
  }, [clearHoverTimer, closeWithDelay, open]);

  React.useEffect(() => {
    if (!open || detail?.coordinate === skill.coordinate) {
      return;
    }

    let active = true;
    setPreviewState("loading");
    void resolveRelaySkills([skill.coordinate])
      .then(([resolved]) => {
        if (!active) return;
        setDetail(resolved ?? null);
        setPreviewState(resolved ? "ready" : "error");
      })
      .catch(() => {
        if (!active) return;
        setDetail(null);
        setPreviewState("error");
      });

    return () => {
      active = false;
    };
  }, [detail?.coordinate, open, skill.coordinate]);

  const displayTitle = detail?.title || skill.title || skill.slug;
  const displaySummary = detail?.summary || skill.summary;
  const focusPreviewOnTab = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Tab" || event.shiftKey || !open) return;
    event.preventDefault();
    contentRef.current?.focus();
  };
  const childTrigger =
    asChild && React.isValidElement(children)
      ? React.cloneElement(
          children as React.ReactElement<Record<string, unknown>>,
          {
            onBlur: closeWithDelay,
            onClick: () => {
              clearHoverTimer();
              setOpen(false);
            },
            onFocus: () => {
              clearHoverTimer();
              if (previewOnFocus) openPreview();
            },
            onKeyDown: focusPreviewOnTab,
            onMouseEnter: openWithDelay,
            onMouseLeave: closeWithDelay,
            ref: (node: HTMLElement | null) => {
              triggerRef.current = node;
            },
          },
        )
      : null;

  return (
    <Popover
      onOpenChange={(nextOpen) => {
        if (nextOpen) openPreview();
      }}
      open={open}
    >
      <PopoverAnchor asChild>
        {childTrigger ?? (
          <button
            {...buttonProps}
            onBlur={(event) => {
              onBlur?.(event);
              closeWithDelay();
            }}
            onClick={(event) => {
              clearHoverTimer();
              setOpen(false);
              onClick?.(event);
            }}
            onFocus={(event) => {
              onFocus?.(event);
              clearHoverTimer();
              if (previewOnFocus) openPreview();
            }}
            onKeyDown={focusPreviewOnTab}
            onMouseEnter={(event) => {
              onMouseEnter?.(event);
              openWithDelay();
            }}
            onMouseLeave={(event) => {
              onMouseLeave?.(event);
              closeWithDelay();
            }}
            ref={(node) => {
              triggerRef.current = node;
            }}
            type="button"
          >
            {children}
          </button>
        )}
      </PopoverAnchor>
      <PopoverContent
        ref={contentRef}
        align={previewAlign}
        aria-label={`${displayTitle} shared instruction preview`}
        className="flex max-h-[min(18rem,var(--radix-popover-content-available-height))] w-[min(28rem,calc(100vw-2rem))] max-w-none flex-col overflow-hidden p-0"
        data-testid="relay-skill-hover-card"
        onBlur={closeWithDelay}
        onEscapeKeyDown={() => setOpen(false)}
        onFocus={clearHoverTimer}
        onKeyDown={(event) => {
          if (
            event.key !== "Tab" ||
            !event.shiftKey ||
            event.target !== event.currentTarget
          )
            return;
          event.preventDefault();
          triggerRef.current?.focus();
        }}
        onOpenAutoFocus={(event) => event.preventDefault()}
        role="region"
        side={previewSide}
        sideOffset={10}
        tabIndex={0}
      >
        <PortalledScrollArea
          className="min-h-24 flex-1 overflow-y-auto overscroll-contain"
          data-testid="relay-skill-hover-card-content"
        >
          <div className="px-4 pb-2 pt-3">
            <div className="flex items-center justify-between gap-3">
              <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                <BookOpen className="h-4 w-4" /> Shared instruction
              </div>
              {onEdit && detail?.editable ? (
                <Button
                  className="h-7 gap-1.5 px-2 text-xs"
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    setOpen(false);
                    onEdit(detail);
                  }}
                  size="sm"
                  type="button"
                  variant="ghost"
                >
                  <Pencil className="h-3.5 w-3.5" />
                  Edit
                </Button>
              ) : null}
            </div>
            <p className="mt-1 text-sm font-semibold text-foreground">
              {displayTitle}
            </p>
            {displaySummary ? (
              <p className="mt-1 text-xs leading-5 text-muted-foreground">
                {displaySummary}
              </p>
            ) : null}
          </div>
          <div className="px-4 py-3">
            {previewState === "loading" || previewState === "idle" ? (
              <p className="text-sm text-muted-foreground">
                Loading full instruction…
              </p>
            ) : null}
            {previewState === "error" ? (
              <p className="text-sm text-destructive">
                This instruction couldn’t be opened. It may have been removed or
                replaced.
              </p>
            ) : null}
            {detail ? (
              <pre className="whitespace-pre-wrap break-words font-mono text-xs leading-5 text-foreground">
                {detail.content}
              </pre>
            ) : null}
          </div>
        </PortalledScrollArea>
      </PopoverContent>
    </Popover>
  );
}
