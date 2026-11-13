// Redaction of sensitive form values captured by the recorder (pure, unit tested).

export const SENSITIVE_PATTERN = /pass|secret|token|card|cvv|ssn|otp/i;
export const SECRET_PLACEHOLDER = '{{secret}}';

export interface FieldDescriptor {
  type?: string | null;
  name?: string | null;
  id?: string | null;
  autocomplete?: string | null;
  label?: string | null;
}

/** True when a field's value must never be recorded or logged. */
export function isSensitiveField(field: FieldDescriptor): boolean {
  if ((field.type ?? '').toLowerCase() === 'password') return true;
  return [field.name, field.id, field.autocomplete, field.label].some(
    (v) => typeof v === 'string' && SENSITIVE_PATTERN.test(v),
  );
}
