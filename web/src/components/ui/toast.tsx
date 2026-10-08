import {
  createContext,
  useCallback,
  useContext,
  useState,
  type ReactNode,
} from 'react';
import { CheckCircle2, CircleAlert, X } from 'lucide-react';
import { Button } from '.';
const ToastContext = createContext<
  (text: string, tone?: 'success' | 'error') => void
>(() => undefined);
export const useToast = () => useContext(ToastContext);
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<
    { id: string; text: string; tone: 'success' | 'error' }[]
  >([]);
  const toast = useCallback(
    (text: string, tone: 'success' | 'error' = 'success') => {
      const id = crypto.randomUUID();
      setItems((items) => [...items, { id, text, tone }]);
      setTimeout(
        () => setItems((items) => items.filter((item) => item.id !== id)),
        6000,
      );
    },
    [],
  );
  return (
    <ToastContext.Provider value={toast}>
      {children}
      <div className="toasts" aria-live="polite">
        {items.map((item) => (
          <div className={`toast ${item.tone}`} key={item.id}>
            {item.tone === 'success' ? (
              <CheckCircle2 size={17} />
            ) : (
              <CircleAlert size={17} />
            )}
            <span>{item.text}</span>
            <Button
              variant="ghost"
              aria-label="Dismiss notification"
              onClick={() =>
                setItems((items) =>
                  items.filter((value) => value.id !== item.id),
                )
              }
            >
              <X size={15} />
            </Button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
