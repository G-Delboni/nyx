import { useLayoutEffect, useRef, useState } from "react";

/**
 * Direction E: the 2D portrait (ref-03) in penumbra. The picture stack is designed at 420x232
 * (photo mapping, glow mask, pendant fix, vignette centre) and scaled to the scene width, so
 * every layer keeps its registration at 380 or 420 px.
 */
export function Portrait() {
  const ref = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);

  useLayoutEffect(() => {
    const el = ref.current?.parentElement;
    if (!el) return;
    const update = () => setScale(el.clientWidth / 420);
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return (
    <div ref={ref} className="scene-pic" style={{ transform: `scale(${scale})` }} aria-hidden="true">
      <div className="scene-photo" />
      <div className="scene-rain" />
      <div className="scene-led" />
      <img className="scene-glow" src="/scene/glow.png" alt="" width={840} height={464} decoding="async" fetchPriority="high" />
      <div className="scene-earfix" />
      <div className="scene-earcursor" />
      <div className="scene-vignette" />
    </div>
  );
}
