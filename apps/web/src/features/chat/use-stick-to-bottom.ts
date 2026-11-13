import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

// Distance in px from the bottom that still counts as "following" the stream.
const FOLLOW_THRESHOLD = 48;

/**
 * Keeps a scroll container pinned to the bottom while content grows (e.g. a
 * streaming reply), unless the user has scrolled up to read.
 *
 * Only moving up stops following. Deciding by the distance alone lost the
 * bottom whenever content grew while a smooth scroll was under way, or a
 * code block rendered late: a scroll event then came in far from the end,
 * though nobody had scrolled.
 *
 * @returns A ref for the scroll container (its first child is observed for
 *   size changes), whether it is at the bottom, and a function to jump there.
 */
export function useStickToBottom() {
  const ref = useRef<HTMLDivElement>(null);
  const followingRef = useRef(true);
  const [atBottom, setAtBottom] = useState(true);

  useLayoutEffect(() => {
    const element = ref.current;
    if (element) {
      element.scrollTop = element.scrollHeight;
    }
  }, []);

  useEffect(() => {
    const element = ref.current;
    if (!element) {
      return;
    }
    let lastTop = element.scrollTop;
    const onScroll = () => {
      const top = element.scrollTop;
      const near = element.scrollHeight - top - element.clientHeight < FOLLOW_THRESHOLD;
      if (near) {
        followingRef.current = true;
      } else if (top < lastTop - 1) {
        followingRef.current = false;
      }
      lastTop = top;
      setAtBottom(near);
    };
    element.addEventListener('scroll', onScroll, { passive: true });

    const content = element.firstElementChild;
    const observer = new ResizeObserver(() => {
      if (followingRef.current) {
        element.scrollTop = element.scrollHeight;
        lastTop = element.scrollTop;
      }
    });
    if (content) {
      observer.observe(content);
    }
    // The container shrinks when the on-screen keyboard opens; the newest
    // messages stay in view instead of sliding under the composer.
    observer.observe(element);
    return () => {
      element.removeEventListener('scroll', onScroll);
      observer.disconnect();
    };
  }, []);

  const scrollToBottom = useCallback((behavior: ScrollBehavior = 'smooth') => {
    const element = ref.current;
    if (!element) {
      return;
    }
    followingRef.current = true;
    setAtBottom(true);
    element.scrollTo({ top: element.scrollHeight, behavior });
  }, []);

  return { ref, atBottom, scrollToBottom };
}
