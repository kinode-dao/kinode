// TODO: remove as much as possible of this..
const BASE_URL = "/main:app-store:sys/";

if (window.our) window.our.process = BASE_URL?.replace("/", "");

export const PROXY_TARGET = `${(import.meta.env.VITE_NODE_URL || `http://localhost:8080`).replace(/\/+$/, '')}${BASE_URL}`;

// This env also has BASE_URL which should match the process + package name
export const WEBSOCKET_URL = import.meta.env.DEV
    ? `${PROXY_TARGET.replace('http', 'ws')}`
    : undefined;
