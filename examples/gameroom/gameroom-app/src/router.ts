// Copyright 2018-2022 Cargill Incorporated
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import Vue from 'vue';
import Router from 'vue-router';
import Home from '@/views/Home.vue';
import store from '@/store';

Vue.use(Router);

const router = new Router({
  routes: [
    {
      path: '/',
      name: 'home',
      redirect: () => {
        if (store.getters['user/isLoggedIn']) {
          return {name: 'dashboard'};
        } else {
          return {name: 'welcome'};
        }
      },
    },
    {
      path: '/welcome',
      name: 'welcome',
      component: Home,
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/Login.vue'),
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('@/views/Register.vue'),
    },
    {
      path: '/dashboard',
      component: () => import('@/views/Dashboard.vue'),
      meta: {
        requiresAuth: true,
      },
      children: [
        {
          path: 'home',
          name: 'dashboard',
          component: () => import('@/views/DashboardHome.vue'),
          meta: {
            requiresAuth: true,
            loadingMessage: 'Loading dashboard',
          },
        },
        {
          path: 'invitations',
          name: 'invitations',
          component: () => import('@/views/Invitations.vue'),
          meta: {
            requiresAuth: true,
            loadingMessage: 'Loading invitations',
          },
        },
        {
          path: 'gamerooms/:id',
          name: 'gamerooms',
          component: () => import('@/views/GameroomDetail.vue'),
          meta: {
            requiresAuth: true,
            loadingMessage: 'Loading gameroom',
          },
        },
        {
          path: 'gamerooms/:id/games/:gameNameHash',
          name: 'games',
          component: () => import('@/views/GameDetail.vue'),
          meta: {
            requiresAuth: true,
          },
        },
        {
          path: '/not-found',
          name: 'not-found',
          component: () => import('@/views/NotFound.vue'),
        },
        {
          path: '/server-error',
          name: 'server-error',
          component: () => import('@/views/ServerError.vue'),
        },
        {
          path: '/request-error',
          name: 'request-error',
          component: () => import('@/views/RequestError.vue'),
        },
      ],
    },
  ],
});

router.beforeEach((to, from, next) => {
  if (to && to.meta) {
    store.commit('pageLoading/setPageLoading', to.meta.loadingMessage);
    if (to.meta.requiresAuth) {
      if (!store.getters['user/isLoggedIn']) {
        return next({ name: 'login' });
      } else {
        return next();
      }
    }
    next();
  }
});

router.afterEach((to, from) => {
  store.commit('pageLoading/setPageLoadingComplete');
});

export default router;
