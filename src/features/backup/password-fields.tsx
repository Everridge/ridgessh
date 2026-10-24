import { useId, useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export const MIN_PASSWORD = 8;

/** New password + confirmation. Returns the password once both match and it is long enough. */
export function usePasswordFields(label = "Backup password") {
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const id = useId();
  const tooShort = password.length > 0 && password.length < MIN_PASSWORD;
  const mismatch = repeat.length > 0 && repeat !== password;
  const valid = password.length >= MIN_PASSWORD && password === repeat;

  const fields = (
    <div className="grid gap-3 sm:grid-cols-2">
      <div className="grid gap-1.5">
        <Label htmlFor={`${id}-pw`}>{label}</Label>
        <Input
          id={`${id}-pw`}
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          aria-invalid={tooShort}
        />
        {tooShort && (
          <p className="text-xs text-destructive">At least {MIN_PASSWORD} characters.</p>
        )}
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor={`${id}-repeat`}>Repeat password</Label>
        <Input
          id={`${id}-repeat`}
          type="password"
          autoComplete="new-password"
          value={repeat}
          onChange={(e) => setRepeat(e.target.value)}
          aria-invalid={mismatch}
        />
        {mismatch && <p className="text-xs text-destructive">The passwords differ.</p>}
      </div>
    </div>
  );

  const reset = () => {
    setPassword("");
    setRepeat("");
  };

  return { fields, password: valid ? password : null, reset };
}
