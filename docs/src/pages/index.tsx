import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Translate, {translate} from '@docusaurus/Translate';

import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">
          <Translate id="homepage.tagline">
            Distributed TCP Proxy for Geo-Aware Load Balancing
          </Translate>
        </p>
        <div className={styles.buttons}>
          <Link
            className="button button--secondary button--lg"
            to="/docs">
            <Translate id="homepage.getStarted">Get Started</Translate>
          </Link>
        </div>
      </div>
    </header>
  );
}

function Feature({title, description}: {title: string; description: string}) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center padding-horiz--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

function HomepageFeatures() {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          <Feature
            title={translate({id: 'homepage.feature1.title', message: 'Geo-Aware Routing'})}
            description={translate({id: 'homepage.feature1.description', message: 'Route clients to the nearest backend based on geographic location using MaxMind GeoIP.'})}
          />
          <Feature
            title={translate({id: 'homepage.feature2.title', message: 'High Performance'})}
            description={translate({id: 'homepage.feature2.description', message: 'Written in Rust with Tokio for zero-copy TCP proxying and minimal latency overhead.'})}
          />
          <Feature
            title={translate({id: 'homepage.feature3.title', message: 'Client Affinity'})}
            description={translate({id: 'homepage.feature3.description', message: 'Sticky sessions ensure consistent backend assignment with configurable TTL.'})}
          />
        </div>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title="Home"
      description={translate({id: 'homepage.description', message: 'Distributed TCP Proxy for Geo-Aware Load Balancing'})}>
      <HomepageHeader />
      <main>
        <HomepageFeatures />
      </main>
    </Layout>
  );
}
