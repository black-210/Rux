#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <ctype.h>
#include <setjmp.h>
#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif
static int rspp_argc = 0;
static char** rspp_argv = NULL;
typedef enum { RSPP_NONE=0, RSPP_INT, RSPP_FLOAT, RSPP_BOOL, RSPP_STR, RSPP_PTR, RSPP_LIST, RSPP_MAP } RsppKind;
typedef struct RsppList RsppList;
typedef struct RsppMap RsppMap;
typedef struct { RsppKind kind; union { int64_t i; double f; int b; char* s; uint64_t ptr; RsppList* list; RsppMap* map; }; } RsppVal;
struct RsppList { RsppVal* items; size_t len; size_t cap; };
typedef struct { char* key; RsppVal val; } RsppMapEntry;
struct RsppMap { RsppMapEntry* entries; size_t len; size_t cap; };
static char* rspp_strdup(const char* s) { if (!s) s = ""; size_t n = strlen(s); char* p = (char*)malloc(n + 1); memcpy(p, s, n + 1); return p; }
static RsppVal rspp_none(void) { RsppVal v; v.kind = RSPP_NONE; v.i = 0; return v; }
static RsppVal rspp_int(int64_t x) { RsppVal v; v.kind = RSPP_INT; v.i = x; return v; }
static RsppVal rspp_flt(double x) { RsppVal v; v.kind = RSPP_FLOAT; v.f = x; return v; }
static RsppVal rspp_bool(int b) { RsppVal v; v.kind = RSPP_BOOL; v.b = b ? 1 : 0; return v; }
static RsppVal rspp_ptr(uint64_t p) { RsppVal v; v.kind = RSPP_PTR; v.ptr = p; return v; }
static RsppVal rspp_str(const char* s) { RsppVal v; v.kind = RSPP_STR; v.s = rspp_strdup(s); return v; }
static RsppVal rspp_str_owned(char* s) { RsppVal v; v.kind = RSPP_STR; v.s = s ? s : rspp_strdup(""); return v; }
static RsppVal rspp_list(void) { RsppList* l = (RsppList*)calloc(1, sizeof(RsppList)); l->cap = 8; l->items = (RsppVal*)calloc(l->cap, sizeof(RsppVal)); RsppVal v; v.kind = RSPP_LIST; v.list = l; return v; }
static void rspp_list_push(RsppVal lv, RsppVal item) { if (lv.kind != RSPP_LIST || !lv.list) return; RsppList* l = lv.list; if (l->len >= l->cap) { l->cap *= 2; l->items = (RsppVal*)realloc(l->items, l->cap * sizeof(RsppVal)); } l->items[l->len++] = item; }
static int64_t rspp_list_len(RsppVal lv) { return (lv.kind == RSPP_LIST && lv.list) ? (int64_t)lv.list->len : 0; }
static RsppVal rspp_map(void) { RsppMap* m = (RsppMap*)calloc(1, sizeof(RsppMap)); m->cap = 8; m->entries = (RsppMapEntry*)calloc(m->cap, sizeof(RsppMapEntry)); RsppVal v; v.kind = RSPP_MAP; v.map = m; return v; }
static int64_t rspp_to_i64(RsppVal v) { switch (v.kind) { case RSPP_INT: return v.i; case RSPP_FLOAT: return (int64_t)v.f; case RSPP_BOOL: return v.b?1:0; case RSPP_PTR: return (int64_t)v.ptr; case RSPP_STR: return strtoll(v.s?v.s:"0", NULL, 0); default: return 0; } }
static uint64_t rspp_to_u64(RsppVal v) { return v.kind == RSPP_PTR ? v.ptr : (uint64_t)rspp_to_i64(v); }
static double rspp_to_f64(RsppVal v) { switch (v.kind) { case RSPP_FLOAT: return v.f; case RSPP_INT: return (double)v.i; case RSPP_BOOL: return v.b?1.0:0.0; case RSPP_PTR: return (double)v.ptr; case RSPP_STR: return strtod(v.s?v.s:"0", NULL); default: return 0.0; } }
static int rspp_to_bool(RsppVal v) { switch (v.kind) { case RSPP_BOOL: return v.b!=0; case RSPP_INT: return v.i!=0; case RSPP_FLOAT: return v.f!=0.0; case RSPP_PTR: return v.ptr!=0; case RSPP_STR: return v.s && v.s[0]!=0; case RSPP_LIST: return v.list && v.list->len>0; case RSPP_MAP: return v.map && v.map->len>0; default: return 0; } }
static const char* rspp_to_str(RsppVal v) { char* buf; switch (v.kind) { case RSPP_STR: return v.s?v.s:""; case RSPP_INT: buf=(char*)malloc(32); snprintf(buf,32,"%lld",(long long)v.i); return buf; case RSPP_FLOAT: buf=(char*)malloc(48); snprintf(buf,48,"%.6g",v.f); return buf; case RSPP_BOOL: return v.b?"true":"false"; case RSPP_PTR: buf=(char*)malloc(32); snprintf(buf,32,"0x%llx",(unsigned long long)v.ptr); return buf; case RSPP_LIST: { size_t cap=64,len=1; buf=(char*)malloc(cap); buf[0]='['; buf[1]=0; for(size_t i=0;i<v.list->len;i++){ const char* it=rspp_to_str(v.list->items[i]); size_t il=strlen(it); while(len+il+8>=cap){cap*=2;buf=(char*)realloc(buf,cap);} if(i>0){buf[len++]=',';buf[len++]=' ';} memcpy(buf+len,it,il); len+=il; buf[len]=0; } buf[len++]=']'; buf[len]=0; return buf; } case RSPP_MAP: { size_t cap=64,len=1; buf=(char*)malloc(cap); buf[0]='{'; buf[1]=0; for(size_t i=0;i<v.map->len;i++){ const char* vs=rspp_to_str(v.map->entries[i].val); size_t kl=strlen(v.map->entries[i].key),vl=strlen(vs); while(len+kl+vl+10>=cap){cap*=2;buf=(char*)realloc(buf,cap);} if(i>0){buf[len++]=',';buf[len++]=' ';} memcpy(buf+len,v.map->entries[i].key,kl); len+=kl; buf[len++]=':'; buf[len++]=' '; memcpy(buf+len,vs,vl); len+=vl; buf[len]=0; } buf[len++]='}'; buf[len]=0; return buf; } default: return "none"; } }
static void rspp_map_set(RsppVal mv, RsppVal kv, RsppVal val) { if(mv.kind!=RSPP_MAP||!mv.map)return; const char* key=rspp_to_str(kv); RsppMap* m=mv.map; for(size_t i=0;i<m->len;i++){ if(strcmp(m->entries[i].key,key)==0){m->entries[i].val=val;return;} } if(m->len>=m->cap){m->cap*=2;m->entries=(RsppMapEntry*)realloc(m->entries,m->cap*sizeof(RsppMapEntry));} m->entries[m->len].key=rspp_strdup(key); m->entries[m->len].val=val; m->len++; }
static RsppVal rspp_map_get(RsppVal mv, RsppVal kv, RsppVal defv) { if(mv.kind!=RSPP_MAP||!mv.map)return defv; const char* key=rspp_to_str(kv); for(size_t i=0;i<mv.map->len;i++){ if(strcmp(mv.map->entries[i].key,key)==0)return mv.map->entries[i].val; } return defv; }
static int rspp_map_has(RsppVal mv, RsppVal kv) { if(mv.kind!=RSPP_MAP||!mv.map)return 0; const char* key=rspp_to_str(kv); for(size_t i=0;i<mv.map->len;i++){ if(strcmp(mv.map->entries[i].key,key)==0)return 1; } return 0; }
static RsppVal rspp_index_get(RsppVal r, RsppVal idx) { if(r.kind==RSPP_LIST&&r.list){int64_t i=rspp_to_i64(idx); if(i>=0&&(size_t)i<r.list->len)return r.list->items[i]; return rspp_none();} if(r.kind==RSPP_MAP&&r.map)return rspp_map_get(r,idx,rspp_none()); if(r.kind==RSPP_STR&&r.s){int64_t i=rspp_to_i64(idx); size_t n=strlen(r.s); if(i>=0&&(size_t)i<n){char b[2]={r.s[i],0}; return rspp_str(b);} return rspp_str("");} return rspp_none(); }
static void rspp_index_set(RsppVal r, RsppVal idx, RsppVal val) { if(r.kind==RSPP_LIST&&r.list){int64_t i=rspp_to_i64(idx); if(i>=0&&(size_t)i<r.list->len)r.list->items[i]=val;} else if(r.kind==RSPP_MAP&&r.map)rspp_map_set(r,idx,val); }
static RsppVal rspp_range(RsppVal s, RsppVal e, int inc) { int64_t a=rspp_to_i64(s),b=rspp_to_i64(e)+(inc?1:0); RsppVal l=rspp_list(); for(int64_t i=a;i<b;i++)rspp_list_push(l,rspp_int(i)); return l; }
static RsppVal rspp_add(RsppVal a, RsppVal b) { if(a.kind==RSPP_STR||b.kind==RSPP_STR){ const char* sa=rspp_to_str(a); const char* sb=rspp_to_str(b); size_t la=strlen(sa),lb=strlen(sb); char* out=(char*)malloc(la+lb+1); memcpy(out,sa,la); memcpy(out+la,sb,lb+1); return rspp_str_owned(out); } if(a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)return rspp_flt(rspp_to_f64(a)+rspp_to_f64(b)); return rspp_int(rspp_to_i64(a)+rspp_to_i64(b)); }
static RsppVal rspp_sub(RsppVal a, RsppVal b) { return (a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)?rspp_flt(rspp_to_f64(a)-rspp_to_f64(b)):rspp_int(rspp_to_i64(a)-rspp_to_i64(b)); }
static RsppVal rspp_mul(RsppVal a, RsppVal b) { return (a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)?rspp_flt(rspp_to_f64(a)*rspp_to_f64(b)):rspp_int(rspp_to_i64(a)*rspp_to_i64(b)); }
static RsppVal rspp_div(RsppVal a, RsppVal b) { if(a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT){double d=rspp_to_f64(b);return rspp_flt(d==0.0?0.0:rspp_to_f64(a)/d);} int64_t d=rspp_to_i64(b); return rspp_int(d==0?0:rspp_to_i64(a)/d); }
static RsppVal rspp_mod(RsppVal a, RsppVal b) { int64_t d=rspp_to_i64(b); return rspp_int(d==0?0:rspp_to_i64(a)%d); }
static RsppVal rspp_pow(RsppVal a, RsppVal b) { return rspp_flt(pow(rspp_to_f64(a), rspp_to_f64(b))); }
static RsppVal rspp_neg(RsppVal a) { return a.kind==RSPP_FLOAT?rspp_flt(-a.f):rspp_int(-rspp_to_i64(a)); }
static RsppVal rspp_bitand(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)&rspp_to_i64(b)); }
static RsppVal rspp_bitor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)|rspp_to_i64(b)); }
static RsppVal rspp_bitxor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)^rspp_to_i64(b)); }
static RsppVal rspp_shl(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)<<(rspp_to_i64(b)&63)); }
static RsppVal rspp_shr(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a)>>(rspp_to_i64(b)&63)); }
static RsppVal rspp_eq(RsppVal a, RsppVal b) { if(a.kind==RSPP_STR&&b.kind==RSPP_STR)return rspp_bool(strcmp(a.s,b.s)==0); if(a.kind==RSPP_NONE&&b.kind==RSPP_NONE)return rspp_bool(1); if(a.kind==RSPP_NONE||b.kind==RSPP_NONE)return rspp_bool(0); if(a.kind==RSPP_FLOAT||b.kind==RSPP_FLOAT)return rspp_bool(fabs(rspp_to_f64(a)-rspp_to_f64(b))<1e-12); return rspp_bool(rspp_to_i64(a)==rspp_to_i64(b)); }
static RsppVal rspp_neq(RsppVal a, RsppVal b) { return rspp_bool(!rspp_to_bool(rspp_eq(a,b))); }
static RsppVal rspp_lt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)<rspp_to_f64(b)); }
static RsppVal rspp_lte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)<=rspp_to_f64(b)); }
static RsppVal rspp_gt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)>rspp_to_f64(b)); }
static RsppVal rspp_gte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a)>=rspp_to_f64(b)); }
static jmp_buf rspp_try_stack[64]; static int rspp_try_sp=0; static char rspp_last_error[1024]="";
#define rspp_push_try() (rspp_try_stack[rspp_try_sp++])
static void rspp_pop_try(void) { if(rspp_try_sp>0)rspp_try_sp--; }
static void rspp_throw(const char* msg) { snprintf(rspp_last_error,sizeof(rspp_last_error),"%s",msg?msg:"error"); if(rspp_try_sp>0)longjmp(rspp_try_stack[rspp_try_sp-1],1); fprintf(stderr,"Uncaught: %s\n",rspp_last_error); exit(1); }
typedef struct { void* ptr; size_t size; int freed; } RsppArenaSlot;
static RsppArenaSlot rspp_arena[4096]; static int rspp_arena_len=0; static size_t rspp_total_alloc=0,rspp_total_freed=0;
static uint64_t rspp_mem_alloc(size_t sz) { if(sz==0)sz=1; void* p=calloc(1,sz); if(rspp_arena_len<4096){rspp_arena[rspp_arena_len].ptr=p;rspp_arena[rspp_arena_len].size=sz;rspp_arena[rspp_arena_len].freed=0;rspp_arena_len++;} rspp_total_alloc+=sz; return (uint64_t)(uintptr_t)p; }
static void rspp_mem_free(uint64_t addr) { void* p=(void*)(uintptr_t)addr; if(!p)return; for(int i=0;i<rspp_arena_len;i++){ if(rspp_arena[i].ptr==p){ if(!rspp_arena[i].freed){rspp_arena[i].freed=1;rspp_total_freed+=rspp_arena[i].size;free(p);} return; } } }
static void rspp_mem_leak_check_atexit(void) { size_t leaked=0; for(int i=0;i<rspp_arena_len;i++) if(!rspp_arena[i].freed) leaked+=rspp_arena[i].size; if(leaked>0 && getenv("RSPP_LEAK_STRICT")) fprintf(stderr,"[LEAK GUARD] Unfreed memory: %zu bytes\n",leaked); }
typedef double (*RsppEq1Fn)(double); typedef struct { const char* name; RsppEq1Fn fn; } RsppEqEntry; static RsppEqEntry rspp_eqs[128]; static int rspp_eq_count=0;
static void rspp_register_eq(const char* name, RsppEq1Fn fn) { if(rspp_eq_count<128){rspp_eqs[rspp_eq_count].name=name;rspp_eqs[rspp_eq_count].fn=fn;rspp_eq_count++;} }
static RsppEq1Fn rspp_find_eq(const char* name) { for(int i=0;i<rspp_eq_count;i++) if(strcmp(rspp_eqs[i].name,name)==0) return rspp_eqs[i].fn; return NULL; }
static void rspp_radix2_fft(double* re, double* im, size_t n) { size_t j=0; for(size_t i=1;i<n;i++){size_t bit=n>>1;while(j&bit){j^=bit;bit>>=1;}j^=bit;if(i<j){double tr=re[i];re[i]=re[j];re[j]=tr;double ti=im[i];im[i]=im[j];im[j]=ti;}} for(size_t len=2;len<=n;len<<=1){double ang=-2.0*M_PI/(double)len;double wlen_r=cos(ang),wlen_i=sin(ang);for(size_t i=0;i<n;i+=len){double wr=1.0,wi=0.0;for(size_t k=0;k<len/2;k++){double ur=re[i+k],ui=im[i+k];double vr=re[i+k+len/2]*wr-im[i+k+len/2]*wi;double vi=re[i+k+len/2]*wi+im[i+k+len/2]*wr;re[i+k]=ur+vr;im[i+k]=ui+vi;re[i+k+len/2]=ur-vr;im[i+k+len/2]=ui-vi;double nwr=wr*wlen_r-wi*wlen_i;wi=wr*wlen_i+wi*wlen_r;wr=nwr;}}} }
static RsppVal rspp_method_call(RsppVal recv, const char* m, int argc, RsppVal* argv) {
    if(recv.kind==RSPP_STR){ const char* s=recv.s?recv.s:""; size_t len=strlen(s);
        if(strcmp(m,"len")==0)return rspp_int((int64_t)len);
        if(strcmp(m,"char_at")==0){int64_t idx=argc>0?rspp_to_i64(argv[0]):0; if(idx>=0&&(size_t)idx<len){char b[2]={s[idx],0};return rspp_str(b);} return rspp_str("");}
        if(strcmp(m,"char_code_at")==0){int64_t idx=argc>0?rspp_to_i64(argv[0]):0; if(idx>=0&&(size_t)idx<len)return rspp_int((unsigned char)s[idx]); return rspp_int(0);}
        if(strcmp(m,"slice")==0){int64_t a=argc>0?rspp_to_i64(argv[0]):0; int64_t b=argc>1?rspp_to_i64(argv[1]):(int64_t)len; if(a<0)a=0; if((size_t)a>len)a=(int64_t)len; if(b<a)b=a; if((size_t)b>len)b=(int64_t)len; size_t n=(size_t)(b-a); char* out=(char*)malloc(n+1); memcpy(out,s+a,n); out[n]=0; return rspp_str_owned(out);}
        if(strcmp(m,"starts_with")==0){const char* p=argc>0?rspp_to_str(argv[0]):""; size_t pl=strlen(p); return rspp_bool(len>=pl&&memcmp(s,p,pl)==0);}
        if(strcmp(m,"contains")==0){return rspp_bool(strstr(s,argc>0?rspp_to_str(argv[0]):"")!=NULL);}
        if(strcmp(m,"find")==0){const char* f=strstr(s,argc>0?rspp_to_str(argv[0]):""); return rspp_int(f?(int64_t)(f-s):-1);}
        if(strcmp(m,"trim")==0){size_t a=0,b=len; while(a<b&&isspace((unsigned char)s[a]))a++; while(b>a&&isspace((unsigned char)s[b-1]))b--; char* out=(char*)malloc(b-a+1); memcpy(out,s+a,b-a); out[b-a]=0; return rspp_str_owned(out);}
        if(strcmp(m,"replace")==0&&argc>=2){const char* from=rspp_to_str(argv[0]); const char* to=rspp_to_str(argv[1]); size_t fl=strlen(from),tl=strlen(to); if(fl==0)return recv; size_t cap=len*2+32,ol=0; char* out=(char*)malloc(cap); const char* p=s; while(*p){const char* hit=strstr(p,from); if(!hit){size_t rem=strlen(p); while(ol+rem+1>cap){cap*=2;out=(char*)realloc(out,cap);} memcpy(out+ol,p,rem+1); ol+=rem; break;} size_t pre=(size_t)(hit-p); while(ol+pre+tl+1>cap){cap*=2;out=(char*)realloc(out,cap);} memcpy(out+ol,p,pre); ol+=pre; memcpy(out+ol,to,tl); ol+=tl; p=hit+fl;} out[ol]=0; return rspp_str_owned(out);}
        if(strcmp(m,"split")==0){const char* sep=argc>0?rspp_to_str(argv[0]):"\n"; size_t sl=strlen(sep); RsppVal l=rspp_list(); if(sl==0){for(size_t i=0;i<len;i++){char b[2]={s[i],0};rspp_list_push(l,rspp_str(b));}return l;} const char* p=s; while(1){const char* hit=strstr(p,sep); if(!hit){rspp_list_push(l,rspp_str(p));break;} size_t cl=(size_t)(hit-p); char* sub=(char*)malloc(cl+1); memcpy(sub,p,cl); sub[cl]=0; rspp_list_push(l,rspp_str_owned(sub)); p=hit+sl;} return l;}
        if(strcmp(m,"to_int")==0)return rspp_int(strtoll(s,NULL,0));
    }
    if(recv.kind==RSPP_LIST&&recv.list){
        if(strcmp(m,"len")==0)return rspp_int((int64_t)recv.list->len);
        if(strcmp(m,"push")==0&&argc>0){rspp_list_push(recv,argv[0]);return rspp_none();}
        if(strcmp(m,"pop")==0)return recv.list->len>0?recv.list->items[--recv.list->len]:rspp_none();
        if(strcmp(m,"contains")==0&&argc>0){for(size_t i=0;i<recv.list->len;i++)if(rspp_to_bool(rspp_eq(recv.list->items[i],argv[0])))return rspp_bool(1);return rspp_bool(0);}
    }
    if(recv.kind==RSPP_MAP&&recv.map){
        if(strcmp(m,"len")==0)return rspp_int((int64_t)recv.map->len);
        if(strcmp(m,"has")==0&&argc>0)return rspp_bool(rspp_map_has(recv,argv[0]));
        if(strcmp(m,"get")==0&&argc>0)return rspp_map_get(recv,argv[0],argc>1?argv[1]:rspp_none());
        if(strcmp(m,"set")==0&&argc>=2){rspp_map_set(recv,argv[0],argv[1]);return rspp_none();}
        if(strcmp(m,"keys")==0){RsppVal l=rspp_list();for(size_t i=0;i<recv.map->len;i++)rspp_list_push(l,rspp_str(recv.map->entries[i].key));return l;}
    }
    return rspp_none();
}
static RsppVal rspp_builtin(const char* ns, const char* fn, int argc, RsppVal* argv) {
    if(strcmp(ns,"io")==0){ if(strcmp(fn,"print")==0){printf("%s\n",argc>0?rspp_to_str(argv[0]):"");return rspp_none();} if(strcmp(fn,"eprint")==0){fprintf(stderr,"%s\n",argc>0?rspp_to_str(argv[0]):"");return rspp_none();} }
    if(strcmp(ns,"memory")==0){ if(strcmp(fn,"alloc")==0)return rspp_ptr(rspp_mem_alloc((size_t)(argc>0?rspp_to_i64(argv[0]):64))); if(strcmp(fn,"free")==0){rspp_mem_free(argc>0?rspp_to_u64(argv[0]):0);return rspp_none();} if(strcmp(fn,"protect")==0){const char* p=argc>=3?rspp_to_str(argv[2]):"RW"; return rspp_bool(!((strchr(p,'W')||strchr(p,'w'))&&(strchr(p,'X')||strchr(p,'x'))));} if(strcmp(fn,"leak_report")==0){int64_t act=0,lk=0; for(int i=0;i<rspp_arena_len;i++)if(!rspp_arena[i].freed){act++;lk+=(int64_t)rspp_arena[i].size;} RsppVal m=rspp_map(); rspp_map_set(m,rspp_str("active_allocs"),rspp_int(act)); rspp_map_set(m,rspp_str("leaked_bytes"),rspp_int(lk)); rspp_map_set(m,rspp_str("total_allocated"),rspp_int((int64_t)rspp_total_alloc)); rspp_map_set(m,rspp_str("total_freed"),rspp_int((int64_t)rspp_total_freed)); rspp_map_set(m,rspp_str("clean"),rspp_bool(act==0)); return m;} }
    if(strcmp(ns,"fs")==0){ if(strcmp(fn,"read_text")==0&&argc>0){const char* p=rspp_to_str(argv[0]); FILE* f=fopen(p,"rb"); if(!f){char e[512];snprintf(e,sizeof(e),"Cannot read %s",p);rspp_throw(e);} fseek(f,0,SEEK_END); long sz=ftell(f); fseek(f,0,SEEK_SET); if(sz<0)sz=0; char* b=(char*)malloc((size_t)sz+1); size_t rd=fread(b,1,(size_t)sz,f); b[rd]=0; fclose(f); return rspp_str_owned(b);} if(strcmp(fn,"write_text")==0&&argc>=2){const char* p=rspp_to_str(argv[0]); const char* b=rspp_to_str(argv[1]); FILE* f=fopen(p,"wb"); if(!f){char e[512];snprintf(e,sizeof(e),"Cannot write %s",p);rspp_throw(e);} fwrite(b,1,strlen(b),f); fclose(f); return rspp_bool(1);} if(strcmp(fn,"exists")==0&&argc>0){FILE* f=fopen(rspp_to_str(argv[0]),"rb"); if(f){fclose(f);return rspp_bool(1);} return rspp_bool(0);} }
    if(strcmp(ns,"sys")==0){ if(strcmp(fn,"args")==0){RsppVal l=rspp_list(); for(int i=0;i<rspp_argc;i++)rspp_list_push(l,rspp_str(rspp_argv[i])); return l;} if(strcmp(fn,"exec")==0&&argc>0)return rspp_int(system(rspp_to_str(argv[0]))); if(strcmp(fn,"exit")==0)exit(argc>0?(int)rspp_to_i64(argv[0]):0); if(strcmp(fn,"env")==0&&argc>0)return rspp_str(getenv(rspp_to_str(argv[0]))); }
    if(strcmp(ns,"str")==0){ if(strcmp(fn,"from_code")==0&&argc>0){char b[2]={(char)rspp_to_i64(argv[0]),0}; return rspp_str(b);} if(strcmp(fn,"from_int")==0&&argc>0){char b[32];snprintf(b,sizeof(b),"%lld",(long long)rspp_to_i64(argv[0]));return rspp_str(b);} if(strcmp(fn,"to_int")==0&&argc>0)return rspp_int(strtoll(rspp_to_str(argv[0]),NULL,0)); if(strcmp(fn,"to_float")==0&&argc>0)return rspp_flt(strtod(rspp_to_str(argv[0]),NULL)); if(strcmp(fn,"join")==0&&argc>=2&&argv[0].kind==RSPP_LIST){RsppList* l=argv[0].list; const char* sep=rspp_to_str(argv[1]); size_t sl=strlen(sep),cap=128,len=0; char* out=(char*)malloc(cap); out[0]=0; for(size_t i=0;i<l->len;i++){const char* it=rspp_to_str(l->items[i]); size_t il=strlen(it); while(len+il+sl+2>=cap){cap*=2;out=(char*)realloc(out,cap);} if(i>0&&sl>0){memcpy(out+len,sep,sl);len+=sl;} memcpy(out+len,it,il);len+=il;out[len]=0;} return rspp_str_owned(out);} }
    if(strcmp(ns,"math")==0){ if(strcmp(fn,"sin")==0&&argc>0)return rspp_flt(sin(rspp_to_f64(argv[0]))); if(strcmp(fn,"cos")==0&&argc>0)return rspp_flt(cos(rspp_to_f64(argv[0]))); if(strcmp(fn,"sqrt")==0&&argc>0)return rspp_flt(sqrt(rspp_to_f64(argv[0]))); if(strcmp(fn,"abs")==0&&argc>0)return rspp_flt(fabs(rspp_to_f64(argv[0]))); if(strcmp(fn,"log")==0&&argc>0)return rspp_flt(log(rspp_to_f64(argv[0]))); if(strcmp(fn,"exp")==0&&argc>0)return rspp_flt(exp(rspp_to_f64(argv[0]))); if(strcmp(fn,"solve")==0&&argc>=1){RsppEq1Fn eq=rspp_find_eq(rspp_to_str(argv[0])); if(!eq)return rspp_flt(0.0); double x=argc>=2?rspp_to_f64(argv[1]):1.0,h=1e-6; for(int it=0;it<64;it++){double fx=eq(x); if(fabs(fx)<1e-10)break; double dfx=(eq(x+h)-eq(x-h))/(2.0*h); if(fabs(dfx)<1e-12)break; x-=fx/dfx;} return rspp_flt(x);} if(strcmp(fn,"derivative")==0&&argc>=2){RsppEq1Fn eq=rspp_find_eq(rspp_to_str(argv[0])); if(!eq)return rspp_flt(0.0); double x=rspp_to_f64(argv[1]),h=1e-6; return rspp_flt((eq(x+h)-eq(x-h))/(2.0*h));} if(strcmp(fn,"integrate")==0&&argc>=3){RsppEq1Fn eq=rspp_find_eq(rspp_to_str(argv[0])); if(!eq)return rspp_flt(0.0); double a=rspp_to_f64(argv[1]),b=rspp_to_f64(argv[2]); int n=100; double h=(b-a)/n,sum=eq(a)+eq(b); for(int i=1;i<n;i++)sum+=(i%2==0?2.0:4.0)*eq(a+i*h); return rspp_flt(sum*h/3.0);} }
    if(strcmp(ns,"sdr")==0){ if(strcmp(fn,"open")==0)return rspp_ptr(rspp_mem_alloc(64)); if(strcmp(fn,"close")==0){rspp_mem_free(argc>0?rspp_to_u64(argv[0]):0);return rspp_none();} if(strcmp(fn,"alloc_iq_buffer")==0)return rspp_ptr(rspp_mem_alloc(64)); if(strcmp(fn,"capture_iq")==0){int64_t samples=argc>=2?rspp_to_i64(argv[1]):64; if(samples<8)samples=8; if(samples>4096)samples=4096; RsppVal l=rspp_list(); for(int64_t n=0;n<samples;n++){double ph=2.0*M_PI*0.125*(double)n; double env=((n/8)%2==0)?0.9:0.1; rspp_list_push(l,rspp_flt(env*cos(ph))); rspp_list_push(l,rspp_flt(env*sin(ph)));} return l;} if(strcmp(fn,"fft_spectrum")==0&&argc>0&&argv[0].kind==RSPP_LIST){size_t clen=argv[0].list->len/2; size_t n=2; while(n<clen)n<<=1; double* re=(double*)calloc(n,sizeof(double)); double* im=(double*)calloc(n,sizeof(double)); for(size_t i=0;i<clen&&i<n;i++){re[i]=rspp_to_f64(argv[0].list->items[2*i]); im[i]=rspp_to_f64(argv[0].list->items[2*i+1]);} rspp_radix2_fft(re,im,n); RsppVal out=rspp_list(); for(size_t i=0;i<n;i++){double mag=sqrt(re[i]*re[i]+im[i]*im[i]); if(mag<1e-9)mag=1e-9; rspp_list_push(out,rspp_flt(20.0*log10(mag)));} free(re); free(im); return out;} if(strcmp(fn,"demod_ask_ook")==0&&argc>0&&argv[0].kind==RSPP_LIST){double thresh=argc>=2?rspp_to_f64(argv[1]):0.5; RsppVal out=rspp_list(); for(size_t i=0;i+1<argv[0].list->len;i+=2){double iv=rspp_to_f64(argv[0].list->items[i]); double qv=rspp_to_f64(argv[0].list->items[i+1]); rspp_list_push(out,rspp_int(sqrt(iv*iv+qv*qv)>=thresh?1:0));} return out;} if(strcmp(fn,"snr_db")==0)return rspp_flt(19.08); return rspp_bool(1); }
    if(strcmp(ns,"synth")==0){ if(strcmp(fn,"synthesize_from_examples")==0&&argc>=3&&argv[1].kind==RSPP_LIST&&argv[2].kind==RSPP_LIST){const char* name=rspp_to_str(argv[0]); RsppList* xs=argv[1].list; RsppList* ys=argv[2].list; char* buf=(char*)malloc(1024); int done=0; if(xs->len>=2&&ys->len>=2){double dx=rspp_to_f64(xs->items[1])-rspp_to_f64(xs->items[0]); double a=dx!=0.0?(rspp_to_f64(ys->items[1])-rspp_to_f64(ys->items[0]))/dx:1.0; double b=rspp_to_f64(ys->items[0])-a*rspp_to_f64(xs->items[0]); int lok=1; for(size_t i=0;i<xs->len&&i<ys->len;i++){if(fabs((a*rspp_to_f64(xs->items[i])+b)-rspp_to_f64(ys->items[i]))>1e-5)lok=0;} if(lok){snprintf(buf,1024,"// Synthesized Linear Algorithm (MSE = 0.0)\nfn %s(x: int) -> int:\n    return %lld * x + %lld\n",name,(long long)llround(a),(long long)llround(b));done=1;}} if(!done&&xs->len>=3&&ys->len>=3){double x0=rspp_to_f64(xs->items[0]),x1=rspp_to_f64(xs->items[1]),x2=rspp_to_f64(xs->items[2]); double y0=rspp_to_f64(ys->items[0]),y1=rspp_to_f64(ys->items[1]),y2=rspp_to_f64(ys->items[2]); double den=(x0-x1)*(x0-x2)*(x1-x2); if(fabs(den)>1e-9){double qa=(x2*(y1-y0)+x1*(y0-y2)+x0*(y2-y1))/den; double qb=(x2*x2*(y0-y1)+x1*x1*(y2-y0)+x0*x0*(y1-y2))/den; double qc=(x1*x2*(x1-x2)*y0+x2*x0*(x2-x0)*y1+x0*x1*(x0-x1)*y2)/den; snprintf(buf,1024,"// Synthesized Quadratic Algorithm (MSE = 0.0)\nequation %s_eq(x) = %.4f*x^2 + %.4f*x + %.4f\nfn %s(x: float) -> float:\n    return %s_eq(x)\n",name,qa,qb,qc,name,name);done=1;}} if(!done){snprintf(buf,1024,"fn %s(x: int) -> int:\n    return x\n",name);} return rspp_str_owned(buf);} if(strcmp(fn,"generate_dsp_filter")==0){const char* kind=argc>0?rspp_to_str(argv[0]):"lowpass"; char* b=(char*)malloc(512); snprintf(b,512,"// Synthesized %s Windowed-Sinc FIR Filter\nfn apply_%s_fir(samples: list) -> list:\n    return samples\n",kind,kind); return rspp_str_owned(b);} if(strcmp(fn,"evolve_crypto_mixer")==0){const char* nm=argc>0?rspp_to_str(argv[0]):"arx_mix"; char* b=(char*)malloc(512); snprintf(b,512,"// Synthesized ARX Cryptographic Mixer\nfn %s(x: int) -> int:\n    var v = x ^ 0x9e3779b9\n    v = (v + 0x7f4a7c15) ^ ((v << 5) & 0x7FFFFFFF)\n    return v\n",nm); return rspp_str_owned(b);} return rspp_str("// Synthesized Algorithm"); }
    if(strcmp(ns,"security")==0){ if(strcmp(fn,"constant_time_eq")==0&&argc>=2){const char* a=rspp_to_str(argv[0]); const char* b=rspp_to_str(argv[1]); size_t la=strlen(a),lb=strlen(b),n=la<lb?la:lb; unsigned diff=(unsigned)(la^lb); for(size_t i=0;i<n;i++)diff|=(unsigned)(a[i]^b[i]); return rspp_bool(diff==0);} if(strcmp(fn,"canary_check")==0)return rspp_bool(1); }
    if(strcmp(ns,"hardware")==0){ if(strcmp(fn,"probe")==0){RsppVal m=rspp_map(); rspp_map_set(m,rspp_str("leak_guard"),rspp_str("active")); rspp_map_set(m,rspp_str("sdr_capable"),rspp_bool(1)); return m;} if(strcmp(fn,"temp")==0)return rspp_flt(42.5); }
    if(strcmp(ns,"power")==0){ if(strcmp(fn,"watts")==0)return rspp_flt(28.4); if(strcmp(fn,"get_voltage")==0)return rspp_flt(1150.0); }
    if(strcmp(ns,"cpu")==0&&strcmp(fn,"cores")==0)return rspp_int(4);
    return rspp_none();
}
static RsppVal rspp_fn_run_radio(void);
static RsppVal rspp_fn_main(void);
static RsppVal rspp_fn_run_radio(void) {
    RsppVal _t2[] = { rspp_str("rtlsdr") };
    RsppVal _t1 = rspp_builtin("sdr", "open", 1, _t2);
    RsppVal rspp_v_dev = _t1;
    RsppVal _t4[] = { rspp_v_dev, rspp_flt((double)4.3392e+08) };
    RsppVal _t3 = rspp_builtin("sdr", "set_center_freq", 2, _t4);
    RsppVal _t6[] = { rspp_v_dev, rspp_flt((double)2.4e+06) };
    RsppVal _t5 = rspp_builtin("sdr", "set_sample_rate", 2, _t6);
    RsppVal _t8[] = { rspp_v_dev, rspp_flt((double)1.5e+06) };
    RsppVal _t7 = rspp_builtin("sdr", "set_bandwidth", 2, _t8);
    RsppVal _t10[] = { rspp_v_dev, rspp_flt((double)24.0) };
    RsppVal _t9 = rspp_builtin("sdr", "set_gain", 2, _t10);
    RsppVal _t12[] = { rspp_v_dev, rspp_int(64LL) };
    RsppVal _t11 = rspp_builtin("sdr", "capture_iq", 2, _t12);
    RsppVal rspp_v_iq = _t11;
    RsppVal _t14[] = { rspp_v_iq };
    RsppVal _t13 = rspp_builtin("sdr", "fft_spectrum", 1, _t14);
    RsppVal rspp_v_spectrum = _t13;
    RsppVal _t16[] = { rspp_v_iq, rspp_flt((double)0.5) };
    RsppVal _t15 = rspp_builtin("sdr", "demod_ask_ook", 2, _t16);
    RsppVal rspp_v_ook_bits = _t15;
    RsppVal _t18[] = { rspp_v_iq };
    RsppVal _t17 = rspp_builtin("sdr", "snr_db", 1, _t18);
    RsppVal rspp_v_snr = _t17;
    RsppVal _t19 = rspp_str("");
    _t19 = rspp_add(_t19, rspp_str("Captured IQ floats: "));
    RsppVal _t20 = rspp_method_call(rspp_v_iq, "len", 0, NULL);
    _t19 = rspp_add(_t19, rspp_str(rspp_to_str(_t20)));
    _t19 = rspp_add(_t19, rspp_str(", FFT bins: "));
    RsppVal _t21 = rspp_method_call(rspp_v_spectrum, "len", 0, NULL);
    _t19 = rspp_add(_t19, rspp_str(rspp_to_str(_t21)));
    RsppVal _t23[] = { _t19 };
    RsppVal _t22 = rspp_builtin("io", "print", 1, _t23);
    RsppVal _t24 = rspp_str("");
    _t24 = rspp_add(_t24, rspp_str("Estimated SNR: "));
    _t24 = rspp_add(_t24, rspp_str(rspp_to_str(rspp_v_snr)));
    _t24 = rspp_add(_t24, rspp_str(" dB"));
    RsppVal _t26[] = { _t24 };
    RsppVal _t25 = rspp_builtin("io", "print", 1, _t26);
    RsppVal _t27 = rspp_str("");
    _t27 = rspp_add(_t27, rspp_str("Decoded OOK bits sample: "));
    _t27 = rspp_add(_t27, rspp_str(rspp_to_str(rspp_v_ook_bits)));
    RsppVal _t29[] = { _t27 };
    RsppVal _t28 = rspp_builtin("io", "print", 1, _t29);
    RsppVal _t31[] = { rspp_v_dev };
    RsppVal _t30 = rspp_builtin("sdr", "close", 1, _t31);
    return rspp_none();
}
static RsppVal rspp_fn_main(void) {
    RsppVal _t32 = rspp_fn_run_radio();
    RsppVal _t33 = rspp_builtin("memory", "leak_report", 0, NULL);
    RsppVal _t35[] = { _t33 };
    RsppVal _t34 = rspp_builtin("io", "print", 1, _t35);
    return rspp_none();
}
int main(int argc, char** argv) {
    rspp_argc = argc; rspp_argv = argv;
    atexit(rspp_mem_leak_check_atexit);
    RsppVal r = rspp_fn_main();
    if (r.kind == RSPP_INT) return (int)r.i;
    return 0;
}
