import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  tutorialSidebar: [
    'intro',
    'getting-started',
    'architecture',
    'wireguard',
    'internals/load-balancer',
    'internals/client-affinity',
    'internals/node-management',
    'deployment/docker',
    'deployment/flyio',
    {
      type: 'category',
      label: 'AWS',
      items: [
        'deployment/aws',
        {
          type: 'category',
          label: 'RDS',
          items: [
            'deployment/aws-rds-wireguard',
            'deployment/rds-benchmark',
          ],
        },
      ],
    },
    'deployment/gcp',
    'configuration',
    'performance',
    'benchmark',
    {
      type: 'category',
      label: 'Roadmap',
      link: {
        type: 'doc',
        id: 'roadmap/index',
      },
      items: [
        'roadmap/phase-1-internal-dns',
        'roadmap/phase-2-corrosion',
        'roadmap/phase-3-auto-discovery',
        'roadmap/phase-4-ipv6',
        'roadmap/phase-5-anycast-bgp',
        'roadmap/phase-6-health-checks',
      ],
    },
  ],
};

export default sidebars;
