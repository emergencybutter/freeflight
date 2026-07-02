/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_FF_API_BASE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
