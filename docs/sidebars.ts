import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  tutorialSidebar: [
    'intro',
    'getting-started',
    'architecture',
    'configuration',
    {
      type: 'category',
      label: 'Deployment',
      items: [
        'deployment/docker',
        'deployment/kubernetes',
      ],
    },
    {
      type: 'category',
      label: 'Internals',
      items: [
        'internals/load-balancer',
        'internals/client-affinity',
        'internals/node-management',
      ],
    },
  ],
};

export default sidebars;
