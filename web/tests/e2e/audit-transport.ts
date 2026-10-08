import type { Page } from '@playwright/test';
export type AuditFault = {
  path: string;
  kind:
    'empty' | 'error' | 'loading' | 'long' | 'expired' | 'offline' | 'expiring';
};
declare global {
  interface Window {
    __uiAuditFault?: AuditFault;
    __uiAuditAutoLogin?: boolean;
  }
}
/** Browser-only transport fault injection. Never ships in the application or mock. */
export async function installAuditTransport(page: Page, fault?: AuditFault) {
  if (fault)
    await page.addInitScript((value) => {
      window.__uiAuditFault = value;
    }, fault);
  await page.route('**/src/api/mock.ts*', async (route) => {
    const response = await route.fetch();
    const original = (await response.text()).replace(
      'export function createMockTransport(',
      'function createOriginalMockTransport(',
    );
    await route.fulfill({
      response,
      body:
        original +
        `\nexport function createMockTransport(options) {
    const original=createOriginalMockTransport(options);
    const ready=window.__uiAuditAutoLogin ? original('/auth/login',{method:'POST',body:JSON.stringify({email:'admin@visp.dev',password:'demo-password'})}) : Promise.resolve();
    return async (path,init)=>{
      await ready;
      const fault=window.__uiAuditFault;
      if (fault && path.startsWith(fault.path)) {
        if(fault.kind==='loading') await new Promise(resolve=>setTimeout(resolve,3500));
        if(fault.kind==='error') throw new ApiError('upstream','Database connection refused. Check cluster health and network reachability.',502);
        if(fault.kind==='expired') {window.dispatchEvent(new Event('vda:session-expired'));throw new ApiError('unauthenticated','Session expired',401);}
        if(fault.kind==='offline') {window.dispatchEvent(new Event('vda:unreachable'));throw new TypeError('Failed to fetch');}
      }
      const value=await original(path,init);
      if(fault && path.startsWith(fault.path)) {
        if(fault.kind==='expiring' && value?.expires_at) return {...value,expires_at:new Date(Date.now()+2000).toISOString()};
        if(fault.kind==='empty') {
          if(path.startsWith('/overview')) return {...value,clusters_total:0,healthy:0,degraded:0,down:0,unknown:0,queries_24h:0,blocked_24h:0,errors_24h:0,pending_approvals:0,recent_queries:[],unhealthy_clusters:[],discovery:null};
          if(path==='/users')return {items:value.items.slice(0,1)};
          if(value && Array.isArray(value.items))return {...value,items:[],next_cursor:null};
          if(value && Array.isArray(value.schemas))return {schemas:[]};
        }
        if(fault.kind==='long' && value && value.rows) return {...value,rows:value.rows.map((row,index)=>row.map((cell,column)=>column===1?'Long text '+('Detailed customer context. '.repeat(80)):cell)),truncated:true};
      }
      return value;
    };
  }`,
    });
  });
}
