"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap } from "@/lib/api/client";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/field";
import { Wordmark } from "@/components/shell/wordmark";

export default function RegisterPage() {
  const router = useRouter();
  const qc = useQueryClient();
  const providers = useQuery({ queryKey: ["auth-providers"], queryFn: () => unwrap(api.GET("/api/v1/auth/providers")) });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const setup = providers.data?.needs_setup;

  async function submit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const f = new FormData(e.currentTarget);
    const password = String(f.get("password"));
    if (password.length < 12) {
      setError("Use at least 12 characters.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await unwrap(
        api.POST("/api/v1/auth/register", {
          body: { email: String(f.get("email")), name: String(f.get("name")), password, workspace_name: (f.get("workspace") as string) || null },
        }),
      );
      await qc.invalidateQueries();
      router.replace("/overview");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  if (providers.data && !providers.data.registration_open && !setup) {
    return (
      <div className="flex flex-col gap-3">
        <h1>Registration is closed</h1>
        <p className="text-ink-2">Ask a workspace admin to add you, then sign in.</p>
        <Link href="/login">Sign in</Link>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <span className="min-[60rem]:hidden">
        <Wordmark />
      </span>
      <div className="flex flex-col gap-1">
        <h1>{setup ? "Set up ORYN" : "Create an account"}</h1>
        <p className="text-ink-2">{setup ? "The first account administers this instance." : "You will get a workspace of your own."}</p>
      </div>
      <form onSubmit={submit} className="flex flex-col gap-1">
        <Field label="Name">{(id) => <Input id={id} name="name" autoComplete="name" required />}</Field>
        <Field label="Email">{(id) => <Input id={id} name="email" type="email" autoComplete="email" required />}</Field>
        <Field label="Password" hint="At least 12 characters." error={error}>
          {(id, d) => <Input id={id} name="password" type="password" autoComplete="new-password" required minLength={12} aria-describedby={d} aria-invalid={!!error} />}
        </Field>
        <Field label="Workspace name" hint="Optional. You can rename it later.">
          {(id, d) => <Input id={id} name="workspace" aria-describedby={d} />}
        </Field>
        <Button type="submit" variant="primary" size="lg" loading={busy} className="mt-2 w-full">
          {setup ? "Create admin account" : "Create account"}
        </Button>
      </form>
      {!setup ? (
        <p className="text-sm text-ink-2">
          Have an account? <Link href="/login">Sign in</Link>
        </p>
      ) : null}
    </div>
  );
}
