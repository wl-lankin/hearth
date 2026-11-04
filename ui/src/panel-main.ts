import { mount } from 'svelte';
import { blockContextMenu } from './no-context-menu';
import './styles.css';
import TrayPanel from './TrayPanel.svelte';

blockContextMenu(document);
mount(TrayPanel, { target: document.getElementById('panel')! });
