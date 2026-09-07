import { Cause } from "effect";

/** Unwrap Effect's default Promise error while preserving domain error messages. */
export function errorMessage(error: unknown): string {
  if (Cause.isUnknownException(error) && "cause" in error)
    return errorMessage(error.cause);
  return error instanceof Error ? error.message : String(error);
}

/** Show failure reasons in notifications without Effect's internal stack frames. */
export function causeMessage(cause: Cause.Cause<unknown>): string {
  const errors = [...Cause.failures(cause), ...Cause.defects(cause)];
  return errors.length > 0
    ? errors.map(errorMessage).join("; ")
    : "Operation interrupted";
}
