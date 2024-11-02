# 使用rust实现的pixiv图片下载

## 下载模式
目前只支持下载特定画师的所有图片

## 获取cookie
进入pixiv网址<https://www.pixiv.net>，然后登陆你自己的账号。

登录成功后按F12打开开发者模式

选择存储 cookie  www.pixiv.net 

在名称中找到PHHSESSID

复制PHPSESSID的值（前面加上PHPSESSID=）

## cookie的导入方式分为两种
1. 在程序运行时按提示输入
2. 在程序同级目录下新建‘pixiv_cookie.txt’文件并将cookie写入   
使用方法1时同样也会创建一个相同文件以方便下次使用
