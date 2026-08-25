import * as React from "react";
import { BookOpen, Check, Plus, X } from "lucide-react";

import {
  createRelaySkill,
  listMyRelaySkills,
  type RelaySkillCover,
  type ResolvedRelaySkill,
  updateRelaySkill,
} from "@/shared/api/tauriPersonas";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { relayInstructionNameFromTitle } from "../lib/relayInstructionName";
import { RelaySkillPreviewButton } from "./RelaySkillPreviewButton";
import { toggleRelaySkillCoordinate } from "./relaySkillPickerState";
import {
  PERSONA_FIELD_CONTROL_CLASS,
  PERSONA_FIELD_SHELL_CLASS,
} from "./agentConfigOptions";

type RelaySkillPickerProps = {
  children: React.ReactNode;
  disabled?: boolean;
  label: string;
  labelFor: string;
  selected: readonly string[];
  onChange: (coordinates: string[]) => void;
};

type LoadState = "loading" | "ready" | "error";

export function RelaySkillPicker({
  children,
  disabled = false,
  label,
  labelFor,
  selected,
  onChange,
}: RelaySkillPickerProps) {
  const [skills, setSkills] = React.useState<RelaySkillCover[]>([]);
  const [loadState, setLoadState] = React.useState<LoadState>("loading");
  const [pickerOpen, setPickerOpen] = React.useState(false);
  const [createOpen, setCreateOpen] = React.useState(false);
  const [editingDetail, setEditingDetail] =
    React.useState<ResolvedRelaySkill | null>(null);
  const [createState, setCreateState] = React.useState<
    "idle" | "publishing" | "error"
  >("idle");
  const [createError, setCreateError] = React.useState("");
  const [draft, setDraft] = React.useState({
    title: "",
    summary: "",
    instructions: "",
  });

  const loadSkills = React.useCallback(async () => {
    setLoadState("loading");
    try {
      setSkills(await listMyRelaySkills());
      setLoadState("ready");
    } catch {
      setLoadState("error");
    }
  }, []);

  React.useEffect(() => {
    void loadSkills();
  }, [loadSkills]);

  const knownCoordinates = React.useMemo(
    () => new Set(skills.map((skill) => skill.coordinate)),
    [skills],
  );
  const unknownSelected =
    loadState === "ready"
      ? selected.filter((coordinate) => !knownCoordinates.has(coordinate))
      : [];
  const selectedSkills = selected
    .map((coordinate) =>
      skills.find((skill) => skill.coordinate === coordinate),
    )
    .filter((skill): skill is RelaySkillCover => skill !== undefined);
  const hasSelectedSkills =
    selectedSkills.length > 0 || unknownSelected.length > 0;

  function toggleSkill(skill: RelaySkillCover) {
    if (disabled) return;
    const next = toggleRelaySkillCoordinate(selected, skill);
    if (
      next.length !== selected.length ||
      next.some((coordinate, index) => coordinate !== selected[index])
    ) {
      onChange(next);
    }
  }

  function updateDraft(field: keyof typeof draft, value: string) {
    setDraft((current) => ({
      ...current,
      [field]: value,
    }));
    setCreateError("");
    setCreateState("idle");
  }

  function openCreateDialog() {
    setDraft({ title: "", summary: "", instructions: "" });
    setEditingDetail(null);
    setCreateError("");
    setCreateState("idle");
    setCreateOpen(true);
  }

  function openEditDialog(detail: ResolvedRelaySkill) {
    setPickerOpen(false);
    setDraft({
      title: detail.title,
      summary: detail.summary ?? "",
      instructions: detail.content,
    });
    setEditingDetail(detail);
    setCreateError("");
    setCreateState("idle");
    setCreateOpen(true);
  }

  async function publishSkill(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setCreateState("publishing");
    setCreateError("");
    try {
      const skill = editingDetail
        ? await updateRelaySkill({
            ...draft,
            coordinate: editingDetail.coordinate,
            expectedEventId: editingDetail.eventId,
          })
        : await createRelaySkill({
            ...draft,
            name: relayInstructionNameFromTitle(draft.title),
          });
      setSkills((current) => [
        skill,
        ...current.filter((item) => item.coordinate !== skill.coordinate),
      ]);
      if (!editingDetail && !selected.includes(skill.coordinate)) {
        onChange([...selected, skill.coordinate]);
      }
      setDraft({ title: "", summary: "", instructions: "" });
      setEditingDetail(null);
      setCreateState("idle");
      setCreateOpen(false);
    } catch (error) {
      setCreateError(
        error instanceof Error
          ? error.message
          : editingDetail
            ? "This instruction couldn’t be updated."
            : "This instruction couldn’t be published.",
      );
      setCreateState("error");
    }
  }

  function renderPickerIngress() {
    if (loadState !== "ready") return null;

    if (skills.length === 0) {
      return (
        <button
          aria-label="Create shared instruction"
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-muted/65 text-foreground hover:bg-muted"
          disabled={disabled}
          onClick={openCreateDialog}
          title="Create shared instruction"
          type="button"
        >
          <BookOpen className="h-3.5 w-3.5" />
        </button>
      );
    }

    return (
      <DropdownMenu
        modal={false}
        onOpenChange={setPickerOpen}
        open={pickerOpen}
      >
        <DropdownMenuTrigger asChild>
          <button
            aria-label="Add shared instructions"
            className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-muted/65 text-foreground hover:bg-muted"
            disabled={disabled}
            title="Browse shared instructions"
            type="button"
          >
            <BookOpen className="h-3.5 w-3.5" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="start"
          className="w-80 max-w-[calc(100vw-2rem)] overflow-hidden p-2"
          onCloseAutoFocus={(event) => event.preventDefault()}
          onFocusOutside={(event) => {
            if (
              event.target instanceof Element &&
              event.target.closest('[data-testid="relay-skill-hover-card"]')
            ) {
              event.preventDefault();
            }
          }}
          onInteractOutside={(event) => {
            if (
              event.target instanceof Element &&
              event.target.closest('[data-testid="relay-skill-hover-card"]')
            ) {
              event.preventDefault();
            }
          }}
          side="left"
          sideOffset={5}
        >
          <div className="max-h-[min(20rem,var(--radix-dropdown-menu-content-available-height))] overflow-y-auto overscroll-contain">
            {skills.map((skill) => (
              <RelaySkillPreviewButton
                asChild
                key={skill.coordinate}
                onEdit={openEditDialog}
                previewSide="left"
                skill={skill}
              >
                <DropdownMenuCheckboxItem
                  checked={selected.includes(skill.coordinate)}
                  className="group gap-2 px-2 py-1 [&>span:first-child]:hidden"
                  disabled={disabled || !skill.compatible}
                  onCheckedChange={() => toggleSkill(skill)}
                  onSelect={(event) => event.preventDefault()}
                >
                  <span className="min-w-0 flex-1">
                    <span
                      className="line-clamp-1 text-xs font-medium text-foreground"
                      data-testid="relay-skill-menu-title"
                    >
                      {skill.title || skill.slug}
                    </span>
                    {skill.summary ? (
                      <span
                        className="mt-0.5 block truncate text-2xs font-light leading-5 text-muted-foreground"
                        data-testid="relay-skill-menu-summary"
                      >
                        {skill.summary}
                      </span>
                    ) : null}
                    {!skill.compatible ? (
                      <span className="mt-0.5 line-clamp-1 text-xs text-destructive">
                        {skill.incompatibilities[0]?.message}
                      </span>
                    ) : null}
                  </span>
                  <span
                    className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-muted text-foreground transition-colors group-focus:bg-muted/80"
                    data-testid="relay-skill-menu-toggle"
                  >
                    {selected.includes(skill.coordinate) ? (
                      <Check className="h-3.5 w-3.5" />
                    ) : (
                      <Plus className="h-3.5 w-3.5" />
                    )}
                  </span>
                </DropdownMenuCheckboxItem>
              </RelaySkillPreviewButton>
            ))}
          </div>
          <Button asChild size="xs" variant="secondary">
            <DropdownMenuItem
              className="mx-1.5 mt-1 min-h-6 w-fit py-1"
              disabled={disabled}
              onSelect={openCreateDialog}
            >
              Create new
            </DropdownMenuItem>
          </Button>
        </DropdownMenuContent>
      </DropdownMenu>
    );
  }

  return (
    <>
      <div className="mb-1.5 flex min-h-6 items-center justify-between gap-2">
        <label
          className="text-sm font-medium text-foreground"
          htmlFor={labelFor}
        >
          {label}
        </label>
        {renderPickerIngress()}
      </div>
      <div
        className={cn(
          PERSONA_FIELD_SHELL_CLASS,
          "grid min-h-40 resize-y overflow-hidden",
          hasSelectedSkills && "[&_textarea]:pb-[3.25rem]",
        )}
        data-testid="relay-skill-picker-field"
      >
        <div className="col-start-1 row-start-1 min-h-0">{children}</div>
        {hasSelectedSkills ? (
          <div
            className="z-10 col-start-1 row-start-1 mr-6 flex max-h-20 flex-wrap items-center gap-1.5 self-end overflow-y-auto px-3 pb-2 pt-4"
            data-testid="relay-skill-picker-selected"
            style={{
              backgroundImage:
                "linear-gradient(to bottom, transparent 0, color-mix(in oklab, hsl(var(--muted)) 40%, hsl(var(--background))) 0.75rem)",
            }}
          >
            {selectedSkills.map((skill) => (
              <div
                className="flex h-7 max-w-full items-center rounded-full border border-border/70 bg-muted/35 pl-2.5 pr-1 text-xs text-foreground"
                key={skill.coordinate}
              >
                <RelaySkillPreviewButton
                  className="max-w-56 truncate"
                  onEdit={openEditDialog}
                  skill={skill}
                >
                  {skill.title || skill.slug}
                </RelaySkillPreviewButton>
                <button
                  aria-label={`Remove ${skill.title || skill.slug}`}
                  className="ml-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground"
                  disabled={disabled}
                  onClick={() =>
                    onChange(
                      selected.filter((value) => value !== skill.coordinate),
                    )
                  }
                  type="button"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            ))}
            {unknownSelected.map((coordinate) => (
              <div
                className="flex h-7 max-w-full items-center rounded-full border border-dashed border-border/70 pl-2.5 pr-1 text-xs text-muted-foreground"
                key={coordinate}
              >
                <span
                  className="max-w-56 truncate font-mono text-xs"
                  title="Instruction unavailable on this relay"
                >
                  Unavailable instruction
                </span>
                <button
                  aria-label="Remove unavailable instruction"
                  className="ml-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full hover:bg-muted hover:text-foreground"
                  disabled={disabled}
                  onClick={() =>
                    onChange(selected.filter((value) => value !== coordinate))
                  }
                  type="button"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            ))}
          </div>
        ) : null}
      </div>

      {loadState === "error" ? (
        <div className="flex items-center justify-between gap-3 pt-2">
          <p className="text-xs text-destructive">
            Shared instructions couldn’t be loaded.
          </p>
          <Button onClick={() => void loadSkills()} size="sm" variant="ghost">
            Try again
          </Button>
        </div>
      ) : null}

      <Dialog
        onOpenChange={(open) => {
          if (createState === "publishing") return;
          setCreateOpen(open);
          if (!open) {
            setCreateError("");
            setCreateState("idle");
            setEditingDetail(null);
          }
        }}
        open={createOpen}
      >
        <DialogContent className="max-h-[min(680px,calc(100vh-2rem))] max-w-xl overflow-y-auto p-5">
          <DialogHeader className="pr-8">
            <DialogTitle>
              {editingDetail
                ? "Edit shared instruction"
                : "Create shared instruction"}
            </DialogTitle>
            <DialogDescription>
              {editingDetail
                ? "Update the reusable guidance agents apply to relevant tasks."
                : "Add reusable guidance agents can apply to relevant tasks."}
            </DialogDescription>
          </DialogHeader>
          <form className="space-y-4" onSubmit={publishSkill}>
            <div className="space-y-4">
              <div className="space-y-1.5">
                <label
                  className="text-sm font-medium"
                  htmlFor="relay-skill-title"
                >
                  Title
                </label>
                <div
                  className={cn(
                    "flex min-h-11 items-center px-3",
                    PERSONA_FIELD_SHELL_CLASS,
                  )}
                >
                  <Input
                    autoFocus
                    className={cn(
                      "h-8 px-0 py-0 leading-6",
                      PERSONA_FIELD_CONTROL_CLASS,
                    )}
                    id="relay-skill-title"
                    maxLength={280}
                    onChange={(event) =>
                      updateDraft("title", event.target.value)
                    }
                    placeholder="Engineering discipline"
                    value={draft.title}
                  />
                </div>
              </div>
              <div className="space-y-1.5">
                <label
                  className="text-sm font-medium"
                  htmlFor="relay-skill-summary"
                >
                  When to use it
                </label>
                <div
                  className={cn("overflow-hidden", PERSONA_FIELD_SHELL_CLASS)}
                >
                  <Textarea
                    className={cn(
                      "resize-none px-3 py-3 leading-5",
                      PERSONA_FIELD_CONTROL_CLASS,
                    )}
                    id="relay-skill-summary"
                    maxLength={1024}
                    onChange={(event) =>
                      updateDraft("summary", event.target.value)
                    }
                    placeholder="Raises implementation quality through validation, self-review, boundary checks, coverage, and second opinions. Use when coding, refactoring, reviewing changes, testing, or preparing work for completion."
                    rows={3}
                    value={draft.summary}
                  />
                </div>
              </div>
              <div className="space-y-1.5">
                <label
                  className="text-sm font-medium"
                  htmlFor="relay-skill-instructions"
                >
                  Instructions
                </label>
                <div
                  className={cn("overflow-hidden", PERSONA_FIELD_SHELL_CLASS)}
                >
                  <Textarea
                    className={cn(
                      "min-h-40 resize-none px-3 py-3 font-mono text-sm leading-5",
                      PERSONA_FIELD_CONTROL_CLASS,
                    )}
                    id="relay-skill-instructions"
                    maxLength={32768}
                    onChange={(event) =>
                      updateDraft("instructions", event.target.value)
                    }
                    placeholder={
                      "Target a 9/10 or better standard for minimalness, elegance, and correctness.\n\nValidate work in the shape the task demands.\nSelf-review for accidental changes, debug code, weak boundaries, missing coverage, and convention violations…"
                    }
                    value={draft.instructions}
                  />
                </div>
              </div>
              {createError ? (
                <p className="text-sm text-destructive" role="alert">
                  {createError}
                </p>
              ) : null}
            </div>
            <DialogFooter>
              <Button
                disabled={createState === "publishing"}
                onClick={() => setCreateOpen(false)}
                type="button"
                variant="outline"
              >
                Cancel
              </Button>
              <Button
                disabled={
                  createState === "publishing" ||
                  !draft.title.trim() ||
                  (!editingDetail &&
                    !relayInstructionNameFromTitle(draft.title)) ||
                  !draft.summary.trim() ||
                  !draft.instructions.trim()
                }
                type="submit"
              >
                {createState === "publishing"
                  ? editingDetail
                    ? "Saving…"
                    : "Publishing…"
                  : editingDetail
                    ? "Save changes"
                    : "Create and add"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
