import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'edgeProxy',
  tagline: 'Distributed TCP Proxy for Geo-Aware Load Balancing',
  favicon: 'img/favicon.ico',

  future: {
    v4: true,
  },

  url: 'https://edgeproxy.edge.cloud',
  baseUrl: '/',

  organizationName: 'edge-cloud',
  projectName: 'edgeproxy',

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'warn',

  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'pt-BR'],
    localeConfigs: {
      en: {
        htmlLang: 'en-US',
        label: 'English',
      },
      'pt-BR': {
        htmlLang: 'pt-BR',
        label: 'Português (Brasil)',
      },
    },
  },

  presets: [
    [
      'classic',
      {
        docs: {
          path: 'markdown',
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/edge-cloud/edgeproxy/tree/main/docs/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/edgeproxy-social-card.png',
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: false,
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'edgeProxy',
      logo: {
        alt: 'edgeProxy Logo',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'tutorialSidebar',
          position: 'left',
          label: 'Documentation',
        },
        {
          type: 'localeDropdown',
          position: 'right',
        },
        {
          href: 'https://github.com/edge-cloud/edgeproxy',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      copyright: `Copyright © ${new Date().getFullYear()} edgeProxy.<br/>Developed and maintained by <a href="https://andrebassi.com.br" target="_blank" rel="noopener noreferrer">André Bassi</a>`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['rust', 'bash', 'sql', 'toml', 'yaml', 'docker'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
