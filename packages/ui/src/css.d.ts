// CSS modules are bundled by the app (Vite); this only tells TypeScript their shape.
declare module '*.module.css' {
  const classes: Readonly<Record<string, string>>;
  export default classes;
}
