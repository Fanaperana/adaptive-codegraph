// TSX EDGE CASES — React patterns that stress the parser

import React, { useState, useCallback, useRef, forwardRef, memo } from "react";

// === 1. Generic component with complex props ===
interface TableProps<T extends Record<string, unknown>> {
  data: T[];
  columns: Array<{
    key: keyof T;
    label: string;
    render?: (value: T[keyof T], row: T) => React.ReactNode;
  }>;
  onRowClick?: (row: T) => void;
  emptyMessage?: string;
}

export function GenericTable<T extends Record<string, unknown>>({
  data,
  columns,
  onRowClick,
  emptyMessage = "No data",
}: TableProps<T>) {
  if (data.length === 0) return <div>{emptyMessage}</div>;

  return (
    <table>
      <thead>
        <tr>
          {columns.map((col) => (
            <th key={String(col.key)}>{col.label}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {data.map((row, i) => (
          <tr key={i} onClick={() => onRowClick?.(row)}>
            {columns.map((col) => (
              <td key={String(col.key)}>
                {col.render
                  ? col.render(row[col.key], row)
                  : String(row[col.key])}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// === 2. forwardRef with generics ===
interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label: string;
  error?: string;
}

export const LabeledInput = forwardRef<HTMLInputElement, InputProps>(
  ({ label, error, ...props }, ref) => (
    <div>
      <label>{label}</label>
      <input ref={ref} {...props} />
      {error && <span className="error">{error}</span>}
    </div>
  )
);
LabeledInput.displayName = "LabeledInput";

// === 3. memo with comparison function ===
interface ExpensiveProps {
  data: number[];
  threshold: number;
}

export const ExpensiveComponent = memo<ExpensiveProps>(
  ({ data, threshold }) => {
    const filtered = data.filter((n) => n > threshold);
    return (
      <ul>
        {filtered.map((n, i) => (
          <li key={i}>{n}</li>
        ))}
      </ul>
    );
  },
  (prev, next) =>
    prev.threshold === next.threshold &&
    prev.data.length === next.data.length
);

// === 4. Render props pattern ===
interface RenderProps<T> {
  data: T;
  children: (data: T) => React.ReactNode;
}

export function DataProvider<T>({ data, children }: RenderProps<T>) {
  return <>{children(data)}</>;
}

// === 5. Higher-order component ===
export function withLoading<P extends object>(
  WrappedComponent: React.ComponentType<P>
) {
  return function WithLoadingComponent(props: P & { isLoading: boolean }) {
    const { isLoading, ...rest } = props;
    if (isLoading) return <div>Loading...</div>;
    return <WrappedComponent {...(rest as P)} />;
  };
}

// === 6. Custom hook returning tuple ===
export function useToggle(
  initial = false
): [boolean, () => void, (v: boolean) => void] {
  const [value, setValue] = useState(initial);
  const toggle = useCallback(() => setValue((v) => !v), []);
  return [value, toggle, setValue];
}

// === 7. Conditional rendering with fragments ===
interface StatusProps {
  status: "loading" | "error" | "success";
  errorMessage?: string;
  children: React.ReactNode;
}

export function StatusWrapper({
  status,
  errorMessage,
  children,
}: StatusProps) {
  return (
    <>
      {status === "loading" && <div className="spinner" />}
      {status === "error" && (
        <div className="error">
          <p>{errorMessage ?? "Unknown error"}</p>
          <button onClick={() => window.location.reload()}>Retry</button>
        </div>
      )}
      {status === "success" && children}
    </>
  );
}

// === 8. useRef with imperative handle ===
export function useInterval(callback: () => void, delay: number | null) {
  const savedCallback = useRef(callback);

  React.useEffect(() => {
    savedCallback.current = callback;
  }, [callback]);

  React.useEffect(() => {
    if (delay === null) return;
    const id = setInterval(() => savedCallback.current(), delay);
    return () => clearInterval(id);
  }, [delay]);
}

// === 9. Component with complex default props ===
const DEFAULT_THEME = {
  primary: "#007bff",
  secondary: "#6c757d",
  fontSize: 14,
} as const;

type Theme = typeof DEFAULT_THEME;

interface ThemeProviderProps {
  theme?: Partial<Theme>;
  children: React.ReactNode;
}

export const ThemeProvider: React.FC<ThemeProviderProps> = ({
  theme,
  children,
}) => {
  const merged = { ...DEFAULT_THEME, ...theme };
  return (
    <div style={{ color: merged.primary, fontSize: merged.fontSize }}>
      {children}
    </div>
  );
};

// === 10. Intersection observer hook ===
export function useIntersectionObserver(
  ref: React.RefObject<HTMLElement>,
  options?: IntersectionObserverInit
): boolean {
  const [isVisible, setIsVisible] = useState(false);

  React.useEffect(() => {
    if (!ref.current) return;
    const observer = new IntersectionObserver(([entry]) => {
      setIsVisible(entry.isIntersecting);
    }, options);
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, [ref, options]);

  return isVisible;
}
