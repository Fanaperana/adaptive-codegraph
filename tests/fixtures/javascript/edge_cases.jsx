// JSX EDGE CASES — tricky React patterns in plain JavaScript

import React, { useState, useEffect, createContext, useContext, useReducer } from "react";

// === 1. Context with complex value ===
const AppContext = createContext(null);

export function AppProvider({ children }) {
  const [theme, setTheme] = useState("light");
  const [locale, setLocale] = useState("en");

  const value = {
    theme,
    setTheme,
    locale,
    setLocale,
    toggleTheme: () => setTheme((t) => (t === "light" ? "dark" : "light")),
  };

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useApp() {
  const context = useContext(AppContext);
  if (!context) throw new Error("useApp must be inside AppProvider");
  return context;
}

// === 2. useReducer with complex action types ===
function todoReducer(state, action) {
  switch (action.type) {
    case "ADD":
      return [...state, { id: Date.now(), text: action.text, done: false }];
    case "TOGGLE":
      return state.map((t) =>
        t.id === action.id ? { ...t, done: !t.done } : t
      );
    case "DELETE":
      return state.filter((t) => t.id !== action.id);
    case "CLEAR_DONE":
      return state.filter((t) => !t.done);
    default:
      return state;
  }
}

export function TodoApp() {
  const [todos, dispatch] = useReducer(todoReducer, []);
  const [input, setInput] = useState("");

  const handleAdd = () => {
    if (input.trim()) {
      dispatch({ type: "ADD", text: input.trim() });
      setInput("");
    }
  };

  return (
    <div>
      <input value={input} onChange={(e) => setInput(e.target.value)} />
      <button onClick={handleAdd}>Add</button>
      <button onClick={() => dispatch({ type: "CLEAR_DONE" })}>Clear Done</button>
      <ul>
        {todos.map((todo) => (
          <li
            key={todo.id}
            style={{ textDecoration: todo.done ? "line-through" : "none" }}
            onClick={() => dispatch({ type: "TOGGLE", id: todo.id })}
          >
            {todo.text}
            <button onClick={() => dispatch({ type: "DELETE", id: todo.id })}>×</button>
          </li>
        ))}
      </ul>
    </div>
  );
}

// === 3. Render prop with HOC combination ===
export function MouseTracker({ children }) {
  const [pos, setPos] = useState({ x: 0, y: 0 });

  useEffect(() => {
    const handler = (e) => setPos({ x: e.clientX, y: e.clientY });
    window.addEventListener("mousemove", handler);
    return () => window.removeEventListener("mousemove", handler);
  }, []);

  return children(pos);
}

// === 4. Component returning null/false/array ===
export function ConditionalRender({ show, items }) {
  if (!show) return null;
  if (!items?.length) return false;

  // Fragment shorthand with array return
  return items.map((item, i) => <span key={i}>{item}</span>);
}

// === 5. Error boundary with hooks workaround ===
export class AsyncBoundary extends React.Component {
  state = { error: null, errorInfo: null };

  static getDerivedStateFromError(error) {
    return { error };
  }

  componentDidCatch(error, errorInfo) {
    this.setState({ errorInfo });
    console.error("AsyncBoundary:", error, errorInfo);
  }

  reset = () => this.setState({ error: null, errorInfo: null });

  render() {
    if (this.state.error) {
      return (
        <div role="alert">
          <h2>Something failed</h2>
          <pre>{this.state.error.toString()}</pre>
          <button onClick={this.reset}>Try Again</button>
        </div>
      );
    }
    return this.props.children;
  }
}

// === 6. Portal-like component ===
export function Modal({ isOpen, onClose, title, children }) {
  if (!isOpen) return null;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <header>
          <h2>{title}</h2>
          <button onClick={onClose} aria-label="Close">×</button>
        </header>
        <main>{children}</main>
      </div>
    </div>
  );
}

// === 7. Compound component pattern ===
function Tabs({ children, defaultTab }) {
  const [activeTab, setActiveTab] = useState(defaultTab);

  return (
    <div className="tabs">
      {React.Children.map(children, (child) =>
        React.cloneElement(child, { activeTab, setActiveTab })
      )}
    </div>
  );
}

Tabs.Tab = function Tab({ id, label, activeTab, setActiveTab }) {
  return (
    <button
      className={activeTab === id ? "active" : ""}
      onClick={() => setActiveTab(id)}
    >
      {label}
    </button>
  );
};

Tabs.Panel = function Panel({ id, activeTab, children }) {
  if (activeTab !== id) return null;
  return <div className="panel">{children}</div>;
};

export { Tabs };

// === 8. List virtualization (simplified) ===
export function VirtualList({ items, itemHeight, containerHeight, renderItem }) {
  const [scrollTop, setScrollTop] = useState(0);
  const startIndex = Math.floor(scrollTop / itemHeight);
  const visibleCount = Math.ceil(containerHeight / itemHeight) + 1;
  const endIndex = Math.min(startIndex + visibleCount, items.length);
  const offsetY = startIndex * itemHeight;

  return (
    <div
      style={{ height: containerHeight, overflow: "auto" }}
      onScroll={(e) => setScrollTop(e.target.scrollTop)}
    >
      <div style={{ height: items.length * itemHeight, position: "relative" }}>
        <div style={{ transform: `translateY(${offsetY}px)` }}>
          {items.slice(startIndex, endIndex).map((item, i) =>
            renderItem(item, startIndex + i)
          )}
        </div>
      </div>
    </div>
  );
}
