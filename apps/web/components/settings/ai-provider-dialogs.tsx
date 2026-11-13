"use client";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap, type Schemas } from "@/lib/api/client";
import { keys, useWsId } from "@/lib/hooks";
import { Dialog } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox, Field, Input, Select } from "@/components/ui/field";

type Kind = Schemas["AiProviderKind"];

export const KINDS: { id: Kind; label: string; key: "required" | "optional" | "none"; baseUrl: "required" | "optional" | "none"; defaults?: boolean }[] = [
  { id: "anthropic", label: "Anthropic", key: "required", baseUrl: "optional", defaults: true },
  { id: "openai", label: "OpenAI", key: "required", baseUrl: "optional" },
  { id: "azure_openai", label: "Azure OpenAI", key: "required", baseUrl: "required" },
  { id: "gemini", label: "Google Gemini", key: "required", baseUrl: "optional" },
  { id: "ollama", label: "Ollama (local)", key: "none", baseUrl: "optional" },
  { id: "openai_compatible", label: "OpenAI-compatible endpoint", key: "optional", baseUrl: "required" },
  { id: "mock", label: "Mock (deterministic, for testing)", key: "none", baseUrl: "none", defaults: true },
];

export const kindLabel = (k: string) => KINDS.find((x) => x.id === k)?.label ?? k;

const BASE_HINT: Partial<Record<Kind, string>> = {
  azure_openai: "Your resource endpoint, for example https://my-resource.openai.azure.com.",
  openai_compatible: "The API root, for example https://llm.internal.example.com/v1.",
  ollama: "Defaults to http://localhost:11434 when empty.",
};

export function NewProviderDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const [kind, setKind] = useState<Kind>("anthropic");
  const [name, setName] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiVersion, setApiVersion] = useState("");
  const [isDefault, setIsDefault] = useState(false);
  const [model, setModel] = useState("");
  const [inCost, setInCost] = useState("");
  const [outCost, setOutCost] = useState("");
  const spec = KINDS.find((k) => k.id === kind)!;
  const reset = () => {
    setKind("anthropic");
    setName("");
    setApiKey("");
    setBaseUrl("");
    setApiVersion("");
    setIsDefault(false);
    setModel("");
    setInCost("");
    setOutCost("");
  };
  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/api/v1/workspaces/{ws}/ai/providers", {
          params: { path: { ws } },
          body: {
            kind,
            name: name.trim(),
            api_key: spec.key !== "none" && apiKey ? apiKey : null,
            base_url: spec.baseUrl !== "none" && baseUrl.trim() ? baseUrl.trim() : null,
            api_version: kind === "azure_openai" && apiVersion.trim() ? apiVersion.trim() : null,
            is_default: isDefault,
            models: model.trim() ? [{ name: model.trim(), input_cost_per_mtok: Number(inCost) || 0, output_cost_per_mtok: Number(outCost) || 0, is_default: true }] : [],
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: keys.ws(ws, "ai") });
      reset();
      onOpenChange(false);
    },
  });
  const valid = name.trim() && (spec.key !== "required" || apiKey) && (spec.baseUrl !== "required" || baseUrl.trim()) && (spec.defaults || model.trim());
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) create.reset();
      }}
      wide
      title="Add AI provider"
      description="Used for planning tests and investigating failures. Keys are encrypted at rest and never shown again."
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" form="new-provider" loading={create.isPending} disabled={!valid}>
            Add provider
          </Button>
        </>
      }
    >
      <form
        id="new-provider"
        className="grid grid-cols-1 gap-x-4 gap-y-2 sm:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (valid) create.mutate();
        }}
      >
        <Field label="Kind">
          {(id) => (
            <Select id={id} value={kind} onChange={(e) => setKind(e.target.value as Kind)}>
              {KINDS.map((k) => (
                <option key={k.id} value={k.id}>
                  {k.label}
                </option>
              ))}
            </Select>
          )}
        </Field>
        <Field label="Name" hint="For example, Anthropic production.">
          {(id, d) => <Input id={id} aria-describedby={d} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" required />}
        </Field>
        {spec.key !== "none" ? (
          <Field label={spec.key === "required" ? "API key" : "API key (optional)"} hint="Stored encrypted. It is never returned by the API." className="sm:col-span-2">
            {(id, d) => <Input id={id} aria-describedby={d} type="password" autoComplete="new-password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} required={spec.key === "required"} />}
          </Field>
        ) : null}
        {spec.baseUrl !== "none" ? (
          <Field label={spec.baseUrl === "required" ? "Base URL" : "Base URL (optional)"} hint={BASE_HINT[kind] ?? "Leave empty for the provider's public endpoint."} className={kind === "azure_openai" ? "" : "sm:col-span-2"}>
            {(id, d) => <Input id={id} aria-describedby={d} type="url" className="font-mono" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} autoComplete="off" required={spec.baseUrl === "required"} />}
          </Field>
        ) : null}
        {kind === "azure_openai" ? (
          <Field label="API version" hint="For example 2024-10-21.">
            {(id, d) => <Input id={id} aria-describedby={d} className="font-mono" value={apiVersion} onChange={(e) => setApiVersion(e.target.value)} autoComplete="off" />}
          </Field>
        ) : null}
        <fieldset className="grid grid-cols-1 gap-x-4 border-t border-rule pt-3 sm:col-span-2 sm:grid-cols-3">
          <legend className="visually-hidden">Model</legend>
          <Field label={spec.defaults ? "Model (optional)" : "Model"} hint={spec.defaults ? "Empty uses the built-in default models." : kind === "azure_openai" ? "Your deployment name." : "For example the model id."} className="sm:col-span-3">
            {(id, d) => <Input id={id} aria-describedby={d} className="font-mono" value={model} onChange={(e) => setModel(e.target.value)} autoComplete="off" required={!spec.defaults} />}
          </Field>
          <Field label="Input price" hint="USD per million tokens.">
            {(id, d) => <Input id={id} aria-describedby={d} inputMode="decimal" className="font-mono" value={inCost} onChange={(e) => setInCost(e.target.value)} placeholder="0" disabled={!model.trim()} />}
          </Field>
          <Field label="Output price" hint="USD per million tokens.">
            {(id, d) => <Input id={id} aria-describedby={d} inputMode="decimal" className="font-mono" value={outCost} onChange={(e) => setOutCost(e.target.value)} placeholder="0" disabled={!model.trim()} />}
          </Field>
        </fieldset>
        <div className="sm:col-span-2">
          <Checkbox label="Use as the workspace default provider" checked={isDefault} onChange={(e) => setIsDefault(e.target.checked)} />
        </div>
        {create.error ? (
          <p role="alert" className="text-sm text-error sm:col-span-2">
            {errorMessage(create.error)}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}

export function EditProviderDialog({ provider: p, onOpenChange }: { provider: Schemas["ProviderView"] | null; onOpenChange: (o: boolean) => void }) {
  const ws = useWsId();
  const qc = useQueryClient();
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState(p?.base_url ?? "");
  const spec = KINDS.find((k) => k.id === p?.kind);
  const save = useMutation({
    mutationFn: () =>
      unwrap(
        api.PATCH("/api/v1/workspaces/{ws}/ai/providers/{pid}", {
          params: { path: { ws, pid: p!.id } },
          body: { api_key: apiKey ? apiKey : undefined, base_url: spec?.baseUrl !== "none" && baseUrl.trim() && baseUrl.trim() !== (p?.base_url ?? "") ? baseUrl.trim() : undefined },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: keys.ws(ws, "ai") });
      onOpenChange(false);
    },
  });
  return (
    <Dialog
      open={!!p}
      onOpenChange={onOpenChange}
      title={p ? `Edit ${p.name}` : "Edit provider"}
      description={p ? kindLabel(p.kind) : undefined}
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" form="edit-provider" loading={save.isPending}>
            Save provider
          </Button>
        </>
      }
    >
      <form
        id="edit-provider"
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        {spec?.key !== "none" ? (
          <Field label="Replace API key" hint={p?.has_api_key ? "A key is set. Leave empty to keep it." : "No key is set."}>
            {(id, d) => <Input id={id} aria-describedby={d} type="password" autoComplete="new-password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} />}
          </Field>
        ) : null}
        {spec?.baseUrl !== "none" ? (
          <Field label="Base URL" hint={(p && BASE_HINT[p.kind as Kind]) ?? "Leave empty for the provider's public endpoint."}>
            {(id, d) => <Input id={id} aria-describedby={d} type="url" className="font-mono" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} autoComplete="off" />}
          </Field>
        ) : null}
        {save.error ? <p role="alert" className="text-sm text-error">{errorMessage(save.error)}</p> : null}
      </form>
    </Dialog>
  );
}
