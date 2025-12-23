#!/usr/bin/env node

const { minify: minifyHtml } = require('html-minifier-terser');
const { minify: minifyJs } = require('terser');
const CleanCSS = require('clean-css');
const MarkdownIt = require('markdown-it');
const fs = require('fs');
const path = require('path');
const MarkdownItAnchor = require('markdown-it-anchor');

// 配置选项
const options = {
  collapseWhitespace: true,
  removeComments: true,
  removeEmptyAttributes: true,
  removeOptionalTags: true,
  removeRedundantAttributes: true,
  removeScriptTypeAttributes: true,
  removeStyleLinkTypeAttributes: true,
  minifyCSS: true,
  minifyJS: true,
  processScripts: ['application/json'],
};

// CSS 压缩选项
const cssOptions = {
  level: 2
};

// 生成 Markdown HTML 模板
function generateMarkdownHtml(title, markdownContent, md) {
  return `
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>${title}</title>
    <style>
        :root {
            --bg-color: #ffffff;
            --text-color: #24292e;
            --code-bg: #f6f8fa;
            --border-color: #dfe2e5;
            --blockquote-color: #6a737d;
        }
        @media (prefers-color-scheme: dark) {
            :root {
                --bg-color: #0d1117;
                --text-color: #c9d1d9;
                --code-bg: #161b22;
                --border-color: #30363d;
                --blockquote-color: #8b949e;
            }
        }
        body {
            max-width: 800px;
            margin: 0 auto;
            padding: 20px;
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
            line-height: 1.6;
            background-color: var(--bg-color);
            color: var(--text-color);
        }
        pre {
            background-color: var(--code-bg);
            padding: 16px;
            border-radius: 6px;
            overflow: auto;
        }
        code {
            background-color: var(--code-bg);
            padding: 2px 4px;
            border-radius: 3px;
        }
        pre code {
            padding: 0;
        }
        img {
            max-width: 100%;
        }
        table {
            border-collapse: collapse;
            width: 100%;
        }
        table td, table th {
            border: 1px solid var(--border-color);
            padding: 6px 13px;
        }
        blockquote {
            border-left: 4px solid var(--border-color);
            margin: 0;
            padding: 0 1em;
            color: var(--blockquote-color);
        }
        a {
            color: #58a6ff;
        }
        /* 标题链接样式 */
        h1:hover .header-anchor,
        h2:hover .header-anchor,
        h3:hover .header-anchor,
        h4:hover .header-anchor,
        h5:hover .header-anchor,
        h6:hover .header-anchor {
            opacity: 1;
        }
        .header-anchor {
            opacity: 0;
            font-size: 0.85em;
            margin-left: 0.25em;
            text-decoration: none;
        }
    </style>
</head>
<body>
    ${md.render(markdownContent)}
</body>
</html>
  `;
}

// 处理文件
async function minifyFile(inputPath, outputPath) {
  try {
    let ext = path.extname(inputPath).toLowerCase();
    const filename = path.basename(inputPath);
    let content = fs.readFileSync(inputPath, 'utf8');
    let minified;

    // 处理 Markdown 文件
    if (ext === '.md') {
      const md = new MarkdownIt({
        html: true,
        linkify: true,
        typographer: true
      }).use(MarkdownItAnchor, {
        slugify: (s) => String(s).trim().toLowerCase().replace(/\s+/g, '-').replace(/[^\w\u4e00-\u9fa5\-]/g, '')
      });

      // 根据文件名确定标题
      const baseName = path.basename(inputPath, '.md');
      const title = baseName.charAt(0).toUpperCase() + baseName.slice(1).toLowerCase();

      // 读取 Markdown 内容
      const markdownPath = inputPath.toLowerCase().endsWith('.md')
        ? inputPath
        : path.join(__dirname, '..', baseName + '.md');
      const markdownContent = fs.readFileSync(markdownPath, 'utf8');

      // 生成 HTML
      content = generateMarkdownHtml(title, markdownContent, md);
      ext = '.html';
    }

    switch (ext) {
      case '.html':
        minified = await minifyHtml(content, options);
        minified = minified.replace(/`([\s\S]*?)`/g, (_match, p1) => {
          return '`' + p1.replace(/\\n\s+/g, '') + '`';
        }).replace(/'([\s\S]*?)'/g, (_match, p1) => {
          return '\'' + p1.replace(/\\n\s+/g, '') + '\'';
        }).replace(/"([\s\S]*?)"/g, (_match, p1) => {
          return '"' + p1.replace(/\\n\s+/g, '') + '"';
        });
        break;
      case '.js':
        const result = await minifyJs(content);
        minified = result.code;
        minified = minified.replace(/`([\s\S]*?)`/g, (_match, p1) => {
          return '`' + p1.replace(/\\n\s+/g, '') + '`';
        }).replace(/'([\s\S]*?)'/g, (_match, p1) => {
          return '\'' + p1.replace(/\\n\s+/g, '') + '\'';
        }).replace(/"([\s\S]*?)"/g, (_match, p1) => {
          return '"' + p1.replace(/\\n\s+/g, '') + '"';
        });
        break;
      case '.css':
        minified = new CleanCSS(cssOptions).minify(content).styles;
        break;
      default:
        throw new Error(`Unsupported file type: ${ext}`);
    }

    fs.writeFileSync(outputPath, minified);
    console.log(`✓ Minified ${path.basename(inputPath)} -> ${path.basename(outputPath)}`);
  } catch (err) {
    console.error(`✗ Error processing ${inputPath}:`, err);
    process.exit(1);
  }
}

// 主函数
async function main() {
  // 获取命令行参数，跳过前两个参数（node和脚本路径）
  const files = process.argv.slice(2);

  if (files.length === 0) {
    console.error('No input files specified');
    process.exit(1);
  }

  const staticDir = path.join(__dirname, '..', 'static');

  for (const file of files) {
    let inputPath;
    let outputPath;

    // 处理 Markdown 文件
    if (file.toLowerCase().endsWith('.md')) {
      const baseName = path.basename(file, '.md').toLowerCase();
      inputPath = path.join(__dirname, '..', file);
      outputPath = path.join(staticDir, `${baseName}.min.html`);
    } else {
      // 处理其他静态文件
      inputPath = path.join(staticDir, file);
      const ext = path.extname(file);
      outputPath = path.join(
        staticDir,
        file.replace(ext, `.min${ext}`)
      );
    }

    await minifyFile(inputPath, outputPath);
  }
}

main();