import { useState } from "react";

export function downloadText(
  name: string,
  content: string,
  type = "application/json",
) {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function CopyButton({ text, label }: { text: string; label: string }) {
  const [message, setMessage] = useState("");
  return (
    <span className="copy-control">
      <button
        type="button"
        className="secondary"
        onClick={() => {
          void (async () => {
            try {
              await navigator.clipboard.writeText(text);
              setMessage("Copied.");
            } catch {
              setMessage("Copy unavailable. Select and copy the text instead.");
            }
          })();
        }}
      >
        {label}
      </button>
      <span role="status">{message}</span>
    </span>
  );
}
