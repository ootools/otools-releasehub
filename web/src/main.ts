import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';

const target = document.getElementById('app');
if (!target) throw new Error('ReleaseHub 缺少应用挂载位置，请重新安装应用。');

const app = mount(App, { target });
export default app;
