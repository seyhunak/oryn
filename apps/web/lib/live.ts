"use client";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

/**
 * Subscribe to an execution's server-sent events while it is active. Each event
 * invalidates the cached execution so the screen re-reads the recorded state; the
 * stream itself is never treated as the record.
 */
export function useExecutionStream(eid: string, active: boolean, onEnd?: () => void) {
  const qc = useQueryClient();
  const [last, setLast] = useState<string | null>(null);
  const endRef = useRef(onEnd);
  useEffect(() => {
    endRef.current = onEnd;
  }, [onEnd]);
  useEffect(() => {
    if (!active || typeof EventSource === "undefined") return;
    const es = new EventSource(`/api/v1/executions/${eid}/events`, { withCredentials: true });
    const refresh = (e: MessageEvent) => {
      setLast(e.data);
      qc.invalidateQueries({ queryKey: ["execution", eid] });
    };
    es.addEventListener("job", refresh);
    es.addEventListener("execution", refresh);
    es.addEventListener("end", () => {
      qc.invalidateQueries({ queryKey: ["execution", eid] });
      endRef.current?.();
      es.close();
    });
    es.onerror = () => {
      // The browser reconnects on its own; a closed stream after the end is expected.
    };
    return () => es.close();
  }, [eid, active, qc]);
  return last;
}
