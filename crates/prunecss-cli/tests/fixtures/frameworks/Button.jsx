import clsx from "clsx";

export function Button({ size, active, disabled, children }) {
  return (
    <button className={clsx("btn", size === "lg" && "btn-lg", { "is-active": active, "is-disabled": disabled })}>
      {children}
    </button>
  );
}
