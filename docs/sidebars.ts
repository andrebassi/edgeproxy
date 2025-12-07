import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  tutorialSidebar: [
    'intro',
    'getting-started',
    {
      type: 'category',
      label: 'Installation',
      collapsed: false,
      items: [
        'deployment/docker',
        'deployment/kubernetes',
        'deployment/aws',
      ],
    },
    'configuration',
    'architecture',
    {
      type: 'category',
      label: 'Internals',
      collapsed: true,
      items: [
        'internals/load-balancer',
        'internals/client-affinity',
        'internals/node-management',
      ],
    },
    'benchmark',
  ],
};

export default sidebars;
