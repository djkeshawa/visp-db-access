import { createContext, useContext } from 'react';

/**
 * The shell's popover layer element. Provided through context (populated by a
 * callback ref) instead of `document.getElementById` during render: on the
 * first render the element does not exist yet, so portals would mount into
 * `<body>` and then jump containers on the next render, detaching open menus.
 */
export const PopoverLayerContext = createContext<HTMLElement | null>(null);

/** Container for Radix portals; `undefined` falls back to `<body>`. */
export function usePopoverLayer(): HTMLElement | undefined {
  return useContext(PopoverLayerContext) ?? undefined;
}
