import { start } from './runtime.js';
start({available:()=>false,request:()=>Promise.reject(Error('Offline pack: BDS connector unavailable'))});
