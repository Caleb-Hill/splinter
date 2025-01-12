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

const path = require('path')
const whitelabelConfig = require('./whitelabel.config')

module.exports = {
  css: {
    loaderOptions: {
      sass: {
        data: `
          @import "@/scss/main.scss";
        `
      }
    }
  },
  devServer: {
    proxy: {
      '^/api': {
        target: 'http://localhost:8000',
        pathRewrite: {'^/api': ''},
        ws: true,
        changeOrigin: true
      },
      '^/ws': {
        target: 'ws://localhost:8000',
        pathRewrite: {'^/ws': ''},
        secure: false,
        ws: true,
        changeOrigin: true
      },
    },
  },
  transpileDependencies: ['vuex-module-decorators'],
  configureWebpack: {
    resolve: {
      alias: {
        'brandVariables': path.resolve(
          __dirname, whitelabelConfig[process.env.VUE_APP_BRAND].scssVariables),
        'brandAssets': path.resolve(
          __dirname, whitelabelConfig[process.env.VUE_APP_BRAND].assets)
      }
    }
  }
};
