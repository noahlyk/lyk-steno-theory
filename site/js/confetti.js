// A short burst of confetti (DOM pieces and a CSS animation, nothing to load).

const COLOURS = ['#e8a24a', '#5b9bd5', '#a07ad8', '#6fae7e', '#e0645c'];

export function confetti(count = 90) {
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  const layer = document.createElement('div');
  layer.className = 'confetti';
  layer.setAttribute('aria-hidden', 'true');
  for (let i = 0; i < count; i += 1) {
    const bit = document.createElement('i');
    bit.style.left = `${Math.random() * 100}%`;
    bit.style.background = COLOURS[i % COLOURS.length];
    bit.style.setProperty('--drift', `${(Math.random() - 0.5) * 40}vw`);
    bit.style.setProperty('--spin', `${(Math.random() - 0.5) * 1440}deg`);
    bit.style.animationDuration = `${2.2 + Math.random() * 1.8}s`;
    bit.style.animationDelay = `${Math.random() * 0.6}s`;
    layer.append(bit);
  }
  document.body.append(layer);
  setTimeout(() => layer.remove(), 5000);
}
