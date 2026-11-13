import { AiTag, Tag } from "@/components/ui/status";

/** AI plans carry the AI tag (no confidence figure exists for a plan); heuristic plans say what they came from. */
export function SourceTag({ source }: { source: string }) {
  if (source === "ai") return <AiTag />;
  if (source === "heuristic") return <Tag>Derived from requirements</Tag>;
  return <Tag>{source}</Tag>;
}
