"use client";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage, unwrap } from "@/lib/api/client";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/field";
import { Wordmark } from "@/components/shell/wordmark";

export default function LoginPage() {
  return (
    <Suspense>
      <Login />
    </Suspense>
  );
}

function Login() {
  const router = useRouter();
  const next = useSearchParams().get("next") || "/overview";
  const qc = useQueryClient();
  const providers = useQuery({ queryKey: ["auth-providers"], queryFn: () => unwrap(api.GET("/api/v1/auth/providers")) });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const needsSetup = providers.data?.needs_setup;
  useEffect(() => {
    if (needsSetup) router.replace("/register");
  }, [needsSetup, router]);

  async function submit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const f = new FormData(e.currentTarget);
    setBusy(true);
    setError(null);
    try {
      await unwrap(api.POST("/api/v1/auth/login", { body: { email: String(f.get("email")), password: String(f.get("password")) } }));
      await qc.invalidateQueries();
      router.replace(next.startsWith("/") ? next : "/overview");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <span className="min-[60rem]:hidden">
        <Wordmark />
      </span>
      <div className="flex flex-col gap-1">
        <h1>Sign in</h1>
        <p className="text-ink-2">Use your ORYN account{providers.data?.oidc ? " or your organisation's sign-in" : ""}.</p>
      </div>
      {providers.data?.oidc ? (
        <a className="btn btn-lg w-full" href={`/api/v1/auth/oidc/login?next=${encodeURIComponent(next)}`}>
          Continue with {providers.data.oidc}
        </a>
      ) : null}
      {providers.data?.password !== false ? (
        <form onSubmit={submit} className="flex flex-col gap-1" noValidate={false}>
          <Field label="Email">{(id) => <Input id={id} name="email" type="email" autoComplete="username" required />}</Field>
          <Field label="Password" error={error}>
            {(id, d) => <Input id={id} name="password" type="password" autoComplete="current-password" required aria-describedby={d} aria-invalid={!!error} />}
          </Field>
          <Button type="submit" variant="primary" size="lg" loading={busy} className="mt-2 w-full">
            Sign in
          </Button>
        </form>
      ) : null}
      {providers.data?.registration_open ? (
        <p className="text-sm text-ink-2">
          New here? <Link href="/register">Create an account</Link>
        </p>
      ) : null}
    </div>
  );
}
