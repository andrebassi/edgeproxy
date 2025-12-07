import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  tutorialSidebar: [
    'intro',
    'getting-started',
    'architecture',
    'internals/load-balancer',
    'internals/client-affinity',
    'internals/node-management',
    'deployment/docker',
    'deployment/flyio',
    'deployment/aws',
    'deployment/gcp',
    'configuration',
    'benchmark',
  ],
};

export default sidebars;
