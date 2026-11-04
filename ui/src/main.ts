import { mount } from 'svelte';
import { blockContextMenu } from './no-context-menu';
import App from './App.svelte';
import './styles.css';

blockContextMenu(document);
mount(App, { target: document.getElementById('app')! });
