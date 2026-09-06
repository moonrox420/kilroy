/// <reference types="vite/client" />

// Vite's `?worker` query suffix — imports a file as a Web Worker constructor.
declare module "*?worker" {
  const workerConstructor: {
    new (options?: { name?: string }): Worker;
  };
  export default workerConstructor;
}

// Tell TS about MonacoEnvironment, which Monaco reads off `self` at load time.
declare global {
  interface Window {
    MonacoEnvironment?: {
      getWorker(workerId: string, label: string): Worker;
    };
  }
  // eslint-disable-next-line no-var
  var MonacoEnvironment: Window["MonacoEnvironment"];
}

export {};
